use super::{format_money, PortfolioSnapshot, Position};
use crate::foundation::{Currency, Money};
use std::collections::{BTreeMap, BTreeSet};

/// Read-only reconciliation. Only explicit canonical account keys support row matching.
pub(super) fn reconciliation(old: &PortfolioSnapshot, new: &PortfolioSnapshot) -> Vec<String> {
    let mut lines = vec![
        "READ-ONLY POSITIONS PREVIEW · ACTIVE IMPORT AND SAVED PATH UNCHANGED".to_owned(),
        format!("OLD {}", old.as_of),
        format!("NEW {}", new.as_of),
        "APPLY ONLY WITH A FRESH PORT IMPORT <FILE.CSV> · ESC TO CLOSE".to_owned(),
        "IMPORTED CASH/MONEY MARKET · AVAILABLE-TO-TRADE/WITHDRAW UNKNOWN".to_owned(),
    ];
    lines.extend(new.valuation.disclosures());
    let currencies = old
        .positions
        .iter()
        .chain(&new.positions)
        .map(|p| p.currency)
        .collect::<BTreeSet<_>>();
    for currency in currencies {
        for (label, cash_only) in [("NAV", false), ("CASH/MONEY MARKET", true)] {
            let before = total(old, currency, cash_only);
            let after = total(new, currency, cash_only);
            let change = before.zip(after).and_then(|(a, b)| b.checked_sub(a));
            let display = |value: Option<i128>| {
                value
                    .map(|value| format_money(Money::from_minor_units(value, currency)))
                    .unwrap_or_else(|| "UNKNOWN (UNPRICED)".to_owned())
            };
            lines.push(format!(
                "{currency} {label}: OLD {} → NEW {} · CHANGE {}",
                display(before),
                display(after),
                display(change)
            ));
        }
    }
    let unmatched = |snapshot: &PortfolioSnapshot| {
        snapshot
            .positions
            .iter()
            .filter(|p| p.account_id.canonical_key().is_none())
            .count()
    };
    lines.push(format!(
        "UNMATCHED IMPORT-LOCAL ROWS: OLD {} · NEW {} · EQUAL ACCOUNT LABELS DO NOT PROVE IDENTITY",
        unmatched(old),
        unmatched(new)
    ));
    let keyed = |snapshot: &PortfolioSnapshot| {
        snapshot
            .positions
            .iter()
            .filter_map(|p| {
                p.account_id
                    .canonical_key()
                    .map(|key| ((key.to_owned(), p.symbol.clone(), p.currency), p.clone()))
            })
            .collect::<BTreeMap<_, _>>()
    };
    let before = keyed(old);
    let after = keyed(new);
    let keys = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for key in keys {
        let describe = |p: &Position| {
            format!(
                "QTY {} · VALUE {}",
                p.quantity_label(),
                p.market_value_label()
            )
        };
        match (before.get(&key), after.get(&key)) {
            (Some(a), Some(b)) if a.quantity != b.quantity || a.market_value != b.market_value => {
                lines.push(format!(
                    "CHANGED {} {} {}: {} → {}",
                    key.0,
                    key.1,
                    key.2,
                    describe(a),
                    describe(b)
                ))
            }
            (None, Some(_)) if unmatched(old) > 0 => lines.push(format!(
                "UNMATCHED NEW CANONICAL {} {} {} · PRIOR IMPORT HAS UNMATCHED ACCOUNTS",
                key.0, key.1, key.2
            )),
            (Some(_), None) if unmatched(new) > 0 => lines.push(format!(
                "UNMATCHED PRIOR CANONICAL {} {} {} · NEW IMPORT HAS UNMATCHED ACCOUNTS",
                key.0, key.1, key.2
            )),
            (None, Some(b)) => lines.push(format!(
                "ADDED CANONICAL {} {} {}: {}",
                key.0,
                key.1,
                key.2,
                describe(b)
            )),
            (Some(a), None) => lines.push(format!(
                "REMOVED CANONICAL {} {} {}: {}",
                key.0,
                key.1,
                key.2,
                describe(a)
            )),
            _ => {}
        }
    }
    lines
}
fn total(snapshot: &PortfolioSnapshot, currency: Currency, cash_only: bool) -> Option<i128> {
    snapshot
        .positions
        .iter()
        .filter(|p| p.currency == currency && (!cash_only || p.cash))
        .try_fold(0_i128, |sum, p| {
            sum.checked_add(p.market_value?.minor_units())
        })
}
