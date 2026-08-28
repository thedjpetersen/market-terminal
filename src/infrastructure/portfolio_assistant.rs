use std::sync::Arc;

use crate::features::{
    assistant::{
        domain::{
            AssistantActivityCurrencyTotal, AssistantActivityEntry, AssistantActivityLedger,
            AssistantContextSnapshot, AssistantPortfolioSnapshot, AssistantPosition,
        },
        AssistantContextQuery,
    },
    portfolio::{format_money, PortfolioRepository},
};

/// Translates Portfolio's public, versioned snapshots into Assistant-owned,
/// preformatted read models. The Assistant context never receives Portfolio
/// domain types or a Portfolio repository capability.
pub struct PortfolioAssistantContextQuery {
    portfolio: Arc<dyn PortfolioRepository>,
}

impl PortfolioAssistantContextQuery {
    pub fn new(portfolio: Arc<dyn PortfolioRepository>) -> Self {
        Self { portfolio }
    }
}

impl AssistantContextQuery for PortfolioAssistantContextQuery {
    fn load_context(&self) -> AssistantContextSnapshot {
        let portfolio = self.portfolio.load_portfolio();
        let activity = self.portfolio.load_activity();
        let net_asset_value = portfolio.net_asset_value_label();
        let available_cash = portfolio.available_cash_label();
        let ytd_return = portfolio.ytd_return_label();
        let sharpe = portfolio.sharpe_label();
        let positions = portfolio
            .positions
            .into_iter()
            .map(|position| {
                let quantity = position.quantity_label();
                let average_cost = position.average_cost_label();
                let market_value = position.market_value_label();
                let pnl = position.pnl_label();
                let weight = position.weight_label();
                AssistantPosition {
                    instrument_id: position.instrument_id.as_str().to_owned(),
                    account: position.account_id.as_str().to_owned(),
                    symbol: position.symbol,
                    quantity,
                    average_cost,
                    market_value,
                    currency: position.currency.to_string(),
                    pnl,
                    weight,
                }
            })
            .collect();
        let net_cash_effect = activity.net_cash_effect_label();
        let entries = activity
            .entries
            .into_iter()
            .map(|entry| {
                let quantity = entry.quantity_label();
                let cash_effect = entry.cash_effect_label();
                let fees = entry.fees_label();
                AssistantActivityEntry {
                    activity_id: entry.activity_id,
                    date: entry.date,
                    account: entry.account_id.as_str().to_owned(),
                    kind: entry.kind.label().to_owned(),
                    symbol: entry.symbol,
                    description: entry.description,
                    quantity,
                    cash_effect,
                    fees,
                    currency: entry.currency.to_string(),
                }
            })
            .collect();
        let currency_totals = activity
            .currency_totals
            .into_iter()
            .map(|total| AssistantActivityCurrencyTotal {
                currency: total.currency.to_string(),
                entries: total.entries,
                inflows: format_money(total.inflows),
                outflows: format_money(total.outflows),
                net_cash_effect: format_money(total.net_cash_effect),
                dividends: format_money(total.dividends),
                interest: format_money(total.interest),
                fees: format_money(total.fees),
                non_cash_entries: total.non_cash_entries,
            })
            .collect();

        AssistantContextSnapshot {
            portfolio: AssistantPortfolioSnapshot {
                source: portfolio.source,
                as_of: portfolio.as_of,
                input_version: portfolio.input_version,
                methodology: portfolio.methodology,
                disclosures: portfolio.disclosures,
                net_asset_value,
                available_cash,
                ytd_return,
                sharpe,
                positions,
            },
            activity: AssistantActivityLedger {
                source: activity.source,
                period: activity.period,
                input_version: activity.input_version,
                methodology: activity.methodology,
                disclosures: activity.disclosures,
                net_cash_effect,
                entries,
                currency_totals,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::DemoData;

    #[test]
    fn translates_portfolio_into_assistant_owned_read_models() {
        let portfolio: Arc<dyn PortfolioRepository> = Arc::new(DemoData);
        let context = PortfolioAssistantContextQuery::new(portfolio).load_context();

        assert_eq!(context.portfolio.positions.len(), 9);
        assert_eq!(context.portfolio.input_version, "DEMO-V1");
        assert!(context
            .portfolio
            .positions
            .iter()
            .all(|position| !position.account.is_empty()));
        assert!(context.activity.entries.is_empty());
        assert_eq!(context.activity.input_version, "—");
        assert_eq!(context.activity.methodology, "NO ACTIVITY INPUT");
    }
}
