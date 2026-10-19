use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};

use super::PortfolioAccountId;

/// Source evidence before position aggregation; dates are never inferred from import time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValuationRow {
    pub row: usize,
    pub account: PortfolioAccountId,
    pub symbol: String,
    pub date: Option<NaiveDate>,
    pub has_basis: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValuationCoverage {
    pub rows: Vec<ValuationRow>,
    pub imported_at: Option<DateTime<Utc>>,
}

impl ValuationCoverage {
    /// Seven calendar days is a disclosure policy, not a claim about quote quality.
    pub const STALE_AFTER_DAYS: i64 = 7;

    pub fn needs_attention(&self, today: NaiveDate) -> bool {
        let first = self.rows.first().and_then(|row| row.date);
        self.rows.iter().any(|row| {
            row.date.is_none()
                || row.date != first
                || row
                    .date
                    .is_some_and(|date| (today - date).num_days() > Self::STALE_AFTER_DAYS)
        })
    }

    pub fn summary(&self, today: NaiveDate) -> String {
        let dates: Vec<_> = self.rows.iter().filter_map(|row| row.date).collect();
        let unknown = self.rows.len() - dates.len();
        let Some(oldest) = dates.iter().min() else {
            return format!("VALUATION UNKNOWN · {unknown}/{} ROWS", self.rows.len());
        };
        let newest = dates.iter().max().expect("nonempty dates");
        let period = if oldest == newest {
            oldest.to_string()
        } else {
            format!("MIXED {oldest}..{newest}")
        };
        let stale = dates
            .iter()
            .filter(|date| (today - **date).num_days() > Self::STALE_AFTER_DAYS)
            .count();
        let warning = if stale > 0 {
            format!("STALE {stale} (>7D) · ")
        } else {
            String::new()
        };
        let coverage = if unknown > 0 {
            format!("{unknown} UNKNOWN")
        } else {
            "ALL DATED".to_owned()
        };
        format!("{warning}VALUATION {period} · {coverage}")
    }

    pub fn basis_label(&self) -> String {
        let basis = self.rows.iter().filter(|row| row.has_basis).count();
        format!("BASIS {basis}/{} ROWS", self.rows.len())
    }

    pub fn disclosures(&self) -> Vec<String> {
        let mut accounts = BTreeMap::<&str, Vec<&ValuationRow>>::new();
        for row in &self.rows {
            accounts.entry(row.account.as_str()).or_default().push(row);
        }
        let mut result = vec![format!(
            "SOURCE COVERAGE · {} · {} ACCOUNT(S)",
            self.basis_label(),
            accounts.len()
        )];
        if let Some(imported) = self.imported_at {
            result.push(format!("IMPORTED AT {imported} · NOT VALUATION TIME"));
        }
        for (account, rows) in accounts {
            let dated = rows.iter().filter(|row| row.date.is_some()).count();
            let oldest = rows.iter().filter_map(|row| row.date).min();
            let newest = rows.iter().filter_map(|row| row.date).max();
            result.push(format!(
                "SOURCE COVERAGE · {account} · {dated}/{} DATED · {}..{}",
                rows.len(),
                oldest
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| "UNKNOWN".into()),
                newest
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| "UNKNOWN".into())
            ));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_threshold_is_pinned_and_unknown_is_not_fresh() {
        let day = NaiveDate::from_ymd_opt(2020, 1, 2).unwrap();
        let mut coverage = ValuationCoverage {
            rows: vec![ValuationRow {
                row: 2,
                account: PortfolioAccountId::new("ACCOUNT 1"),
                symbol: "TEST".into(),
                date: Some(day),
                has_basis: false,
            }],
            imported_at: None,
        };
        assert!(!coverage
            .summary(day + chrono::Duration::days(7))
            .contains("STALE"));
        assert!(coverage
            .summary(day + chrono::Duration::days(8))
            .contains("STALE"));
        assert!(!coverage.needs_attention(day + chrono::Duration::days(7)));
        assert!(coverage.needs_attention(day + chrono::Duration::days(8)));
        coverage.rows[0].date = None;
        assert!(coverage.needs_attention(day));
        assert!(coverage.summary(day).contains("UNKNOWN"));
        assert!(!coverage.summary(day).contains("STALE"));
        assert!(ValuationCoverage::default()
            .summary(day)
            .contains("UNKNOWN"));
    }
}
