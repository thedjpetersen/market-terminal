use crate::features::portfolio::{
    calculate_multi_period_attribution, PortfolioAttributionInput, PortfolioAttributionSnapshot,
    PortfolioError,
};

use super::portfolio_contribution_csv::parse_portfolio_contribution_history_csv;

pub(super) fn parse_portfolio_attribution_csv(
    bytes: &[u8],
    source_name: String,
) -> Result<PortfolioAttributionSnapshot, PortfolioError> {
    let history = parse_portfolio_contribution_history_csv(bytes, source_name)?;
    calculate_multi_period_attribution(PortfolioAttributionInput {
        periods: history.periods,
        source: history.source,
        input_version: history.input_version,
        disclosures: history.disclosures,
    })
    .map_err(|error| PortfolioError::InvalidCsv(format!("ATTRIBUTION INPUT INVALID · {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_contiguous_history_and_links_active_attribution() {
        let csv = b"Account,Symbol,Start Date,End Date,Beginning Value,Ending Value,Benchmark Beginning Value,Benchmark Ending Value,Currency\nBROKER-123,ALPHA,2026-01-01,2026-02-01,600,660,500,525,USD\nBROKER-123,BETA,2026-01-01,2026-02-01,400,440,500,525,USD\nBROKER-123,ALPHA,2026-02-01,2026-03-01,660,594,525,519.75,USD\nBROKER-123,BETA,2026-02-01,2026-03-01,440,484,525,519.75,USD\n";

        let snapshot = parse_portfolio_attribution_csv(csv, "history.csv".to_owned()).unwrap();

        assert_eq!(snapshot.period, "2026-01-01 — 2026-03-01");
        assert_eq!(snapshot.rows.len(), 2);
        assert_eq!(snapshot.rows[0].account_id.as_str(), "ACCOUNT 1");
        assert_eq!(snapshot.currency_totals[0].periods, 2);
        assert_eq!(snapshot.linked_return_label(), "+7.8000%");
        assert_eq!(snapshot.linked_active_return_label(), "+3.8500%");
        assert!(snapshot.input_version.starts_with("CSV-FNV1A64-"));
        assert!(!format!("{snapshot:?}").contains("BROKER-123"));
    }

    #[test]
    fn refuses_single_period_gaps_and_value_discontinuities() {
        let single = b"Symbol,Start Date,End Date,Beginning Value,Ending Value\nA,2026-01-01,2026-02-01,100,110\n";
        let gap = b"Symbol,Start Date,End Date,Beginning Value,Ending Value\nA,2026-01-01,2026-02-01,100,110\nA,2026-02-02,2026-03-01,110,120\n";
        let discontinuity = b"Symbol,Start Date,End Date,Beginning Value,Ending Value\nA,2026-01-01,2026-02-01,100,110\nA,2026-02-01,2026-03-01,109,120\n";
        let unordered = b"Symbol,Start Date,End Date,Beginning Value,Ending Value\nA,2026-02-01,2026-03-01,110,121\nA,2026-01-01,2026-02-01,100,110\n";

        assert!(
            parse_portfolio_attribution_csv(single, "single.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("at least two periods")
        );
        assert!(parse_portfolio_attribution_csv(gap, "gap.csv".to_owned())
            .unwrap_err()
            .to_string()
            .contains("ordered and contiguous"));
        assert!(
            parse_portfolio_attribution_csv(discontinuity, "discontinuous.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("does not reconcile")
        );
        assert!(
            parse_portfolio_attribution_csv(unordered, "unordered.csv".to_owned())
                .unwrap_err()
                .to_string()
                .contains("UNORDERED CONTRIBUTION PERIOD")
        );
    }
}
