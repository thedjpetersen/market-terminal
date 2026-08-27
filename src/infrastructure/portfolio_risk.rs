use std::sync::Arc;

use crate::features::{
    portfolio::PortfolioRepository,
    risk::{
        calculate_risk, RiskCurrencyInput, RiskError, RiskInput, RiskPositionInput, RiskQuery,
        RiskSnapshot,
    },
};

/// Translates Portfolio's versioned public snapshot into Risk-owned inputs.
/// Risk never receives the repository or imports Portfolio domain types.
pub struct PortfolioRiskQuery {
    portfolio: Arc<dyn PortfolioRepository>,
}

impl PortfolioRiskQuery {
    pub fn new(portfolio: Arc<dyn PortfolioRepository>) -> Self {
        Self { portfolio }
    }
}

impl RiskQuery for PortfolioRiskQuery {
    fn load_risk(&self) -> Result<RiskSnapshot, RiskError> {
        let portfolio = self.portfolio.load_portfolio();
        if portfolio.positions.is_empty() {
            return Err(RiskError::Unavailable(portfolio.source));
        }
        let input = RiskInput {
            positions: portfolio
                .positions
                .into_iter()
                .map(|position| RiskPositionInput {
                    instrument_id: position.instrument_id,
                    account: position.account_id.as_str().to_owned(),
                    symbol: position.symbol,
                    currency: position.currency,
                    market_value: position.market_value,
                    cash: position.cash,
                })
                .collect(),
            currencies: portfolio
                .currency_totals
                .into_iter()
                .map(|total| RiskCurrencyInput {
                    currency: total.currency,
                    priced_nav: total.net_asset_value,
                    available_cash: total.available_cash,
                    priced_positions: total.priced_positions,
                    unpriced_positions: total.unpriced_positions,
                })
                .collect(),
            source: format!("VERSIONED PORTFOLIO · {}", portfolio.source),
            as_of: portfolio.as_of,
            input_version: portfolio.input_version,
            disclosures: portfolio.disclosures,
        };
        calculate_risk(input).map_err(|error| RiskError::InvalidInput(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Workspace;
    use crate::features::risk::RiskWorkspace;
    use crate::infrastructure::{CsvPortfolioRepository, DemoData};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn gallery_portfolio_reconciles_before_risk_is_rendered() {
        let portfolio: Arc<dyn PortfolioRepository> = Arc::new(DemoData);
        let snapshot = PortfolioRiskQuery::new(portfolio).load_risk().unwrap();

        assert_eq!(snapshot.currencies.len(), 1);
        assert_eq!(snapshot.currencies[0].priced_positions, 9);
        assert_eq!(snapshot.currencies[0].priced_nav.minor_units(), 104_522_800);
        assert_eq!(
            snapshot.currencies[0].available_cash.minor_units(),
            12_783_400
        );
    }

    #[test]
    #[ignore = "requires MARKET_TERMINAL_PORTFOLIO_CSV pointing to an actual user export"]
    fn actual_configured_portfolio_flows_through_risk_and_terminal_render() {
        let _ = dotenvy::dotenv();
        let portfolio: Arc<dyn PortfolioRepository> = Arc::new(CsvPortfolioRepository::from_env());
        let query: Arc<dyn RiskQuery> = Arc::new(PortfolioRiskQuery::new(portfolio));
        let snapshot = query.load_risk().unwrap();

        assert!(!snapshot.positions.is_empty());
        assert!(!snapshot.currencies.is_empty());
        assert!(snapshot.source.starts_with("VERSIONED PORTFOLIO · CSV ·"));
        assert!(snapshot.input_version.starts_with("CSV-FNV1A64-"));
        assert!(snapshot.methodology.contains("NON-CASH PARALLEL -10%"));
        assert!(snapshot
            .positions
            .iter()
            .all(|position| !position.instrument_id.as_str().starts_with("demo:")));

        let workspace = RiskWorkspace::new(query);
        let backend = TestBackend::new(160, 48);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| workspace.render(frame, frame.area()))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("RISK"));
        assert!(rendered.contains("CSV-FNV1A64"));
        assert!(rendered.contains("-10%"));
    }
}
