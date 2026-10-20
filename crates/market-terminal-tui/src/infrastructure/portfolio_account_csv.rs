//! Raw provider identifiers exist only within one parser invocation.
use crate::features::portfolio::{PortfolioAccountId, PortfolioError};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT_IMPORT: AtomicU64 = AtomicU64::new(1);
pub(super) struct AccountAliases {
    scope: u64,
    raw: BTreeMap<String, PortfolioAccountId>,
    canonical: BTreeMap<String, String>,
}
impl AccountAliases {
    pub fn new() -> Self {
        Self {
            scope: NEXT_IMPORT.fetch_add(1, Ordering::Relaxed),
            raw: BTreeMap::new(),
            canonical: BTreeMap::new(),
        }
    }
    pub fn resolve(
        &mut self,
        raw: &str,
        key: Option<&str>,
    ) -> Result<PortfolioAccountId, PortfolioError> {
        let key = key.map(str::trim).filter(|key| !key.is_empty());
        let fallback = format!("MISSING:{}", key.unwrap_or(""));
        let raw = if matches!(raw, "DEFAULT" | "UNALLOCATED") {
            fallback.as_str()
        } else {
            raw
        };
        if let Some(existing) = self.raw.get(raw) {
            if existing.canonical_key() != key {
                return Err(conflict());
            }
            return Ok(existing.clone());
        }
        let id = if let Some(key) = key {
            if self
                .canonical
                .get(key)
                .is_some_and(|previous| previous != raw)
            {
                return Err(conflict());
            }
            let id = PortfolioAccountId::canonical(key)
                .map_err(|message| PortfolioError::InvalidCsv(message.to_owned()))?;
            // Missing provider account evidence is permitted: the explicit key is the identity.
            self.canonical.insert(key.to_owned(), raw.to_owned());
            id
        } else {
            PortfolioAccountId::import_local(format!("ACCOUNT {}", self.raw.len() + 1), self.scope)
        };
        self.raw.insert(raw.to_owned(), id.clone());
        Ok(id)
    }
}
fn conflict() -> PortfolioError {
    PortfolioError::InvalidCsv("CONFLICTING ACCOUNT KEY MAPPING · EACH SOURCE ACCOUNT MUST HAVE ONE CONSISTENT NONSECRET KEY".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::{
        portfolio_activity_csv::parse_portfolio_activity_csv,
        portfolio_attribution_csv::parse_portfolio_attribution_csv,
        portfolio_contribution_csv::parse_portfolio_contribution_csv,
        portfolio_realized_gain_csv::parse_portfolio_realized_gain_csv,
        portfolio_tax_lot_csv::parse_portfolio_tax_lot_csv,
        portfolio_trade_csv::parse_portfolio_trade_csv,
    };
    #[test]
    fn canonical_key_is_consistent_at_every_parser_boundary() {
        let expected = PortfolioAccountId::canonical("LongTerm").unwrap();
        let activity = parse_portfolio_activity_csv(
            b"Account Key,Date,Amount\nLongTerm,2020-01-01,100\n",
            "test".into(),
        )
        .unwrap();
        let lots = parse_portfolio_tax_lot_csv(b"Account Key,Symbol,Date Acquired,Quantity,Cost Basis\nLongTerm,AAA,2020-01-01,1,100\n", "test".into()).unwrap();
        let closed = parse_portfolio_realized_gain_csv(b"Account Key,Symbol,Date Acquired,Date Sold,Quantity,Proceeds,Cost Basis\nLongTerm,AAA,2020-01-01,2021-01-01,1,110,100\n", "test".into()).unwrap();
        let trades = parse_portfolio_trade_csv(b"Account Key,Trade Date,Action,Symbol,Shares,Price\nLongTerm,2020-01-01,Buy,AAA,1,100\n", "test".into()).unwrap();
        let contribution = parse_portfolio_contribution_csv(b"Account Key,Symbol,Start Date,End Date,Beginning Value,Ending Value\nLongTerm,AAA,2020-01-01,2020-02-01,100,110\n", "test".into()).unwrap();
        let attribution = parse_portfolio_attribution_csv(b"Account Key,Symbol,Start Date,End Date,Beginning Value,Ending Value\nLongTerm,AAA,2020-01-01,2020-02-01,100,110\nLongTerm,AAA,2020-02-01,2020-03-01,110,121\n", "test".into()).unwrap();
        for id in [
            &activity.entries[0].account_id,
            &lots.lots[0].account_id,
            &closed.lots[0].account_id,
            &trades.executions[0].account_id,
            &contribution.rows[0].account_id,
            &attribution.rows[0].account_id,
        ] {
            assert_eq!(id, &expected);
        }
    }
}
