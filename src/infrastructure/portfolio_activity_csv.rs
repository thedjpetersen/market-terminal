use std::collections::BTreeMap;

use chrono::NaiveDate;
use csv::StringRecord;

use crate::{
    features::portfolio::{
        PortfolioAccountId, PortfolioActivityCurrencyTotal, PortfolioActivityEntry,
        PortfolioActivityKind, PortfolioActivityLedger, PortfolioError, PositionQuantity,
    },
    foundation::{Currency, Money},
};

use super::portfolio_csv::{csv_input_version, parse_currency, parse_scaled};

pub(super) const MAX_ACTIVITY_ROWS: usize = 100_000;
pub(super) const MAX_ACTIVITY_COLUMNS: usize = 256;

#[derive(Debug)]
struct ActivityColumns {
    date: usize,
    account: Option<usize>,
    description: Option<usize>,
    category: Option<usize>,
    action: Option<usize>,
    symbol: Option<usize>,
    quantity: Option<usize>,
    amount: Option<usize>,
    fees: Option<usize>,
    currency: Option<usize>,
    cash_export: bool,
}

#[derive(Debug, Default)]
struct AggregateCurrency {
    entries: usize,
    inflows_minor: i128,
    outflows_minor: i128,
    net_cash_effect_minor: i128,
    dividends_minor: i128,
    interest_minor: i128,
    fees_minor: i128,
    non_cash_entries: usize,
}

pub(super) fn parse_portfolio_activity_csv(
    bytes: &[u8],
    source_name: String,
) -> Result<PortfolioActivityLedger, PortfolioError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);
    let records = reader
        .records()
        .take(MAX_ACTIVITY_ROWS + 32)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| PortfolioError::InvalidCsv(format!("CSV PARSE ERROR · {error}")))?;
    if records.len() > MAX_ACTIVITY_ROWS + 31 {
        return Err(PortfolioError::InvalidCsv(format!(
            "ACTIVITY CSV EXCEEDS {MAX_ACTIVITY_ROWS} ROWS"
        )));
    }
    if records
        .iter()
        .any(|record| record.len() > MAX_ACTIVITY_COLUMNS)
    {
        return Err(PortfolioError::InvalidCsv(format!(
            "ACTIVITY CSV EXCEEDS {MAX_ACTIVITY_COLUMNS} COLUMNS"
        )));
    }

    let (header_index, columns) = records
        .iter()
        .take(32)
        .enumerate()
        .find_map(|(index, record)| {
            ActivityColumns::from_header(record).map(|columns| (index, columns))
        })
        .ok_or_else(|| {
            PortfolioError::InvalidCsv(
                "NO ACTIVITY HEADER FOUND · NEED DATE AND AMOUNT, OR DATED SECURITY ACTION"
                    .to_owned(),
            )
        })?;

    let usd = Currency::new("USD").expect("USD is a valid currency");
    let mut entries = Vec::new();
    let mut account_aliases = BTreeMap::<String, PortfolioAccountId>::new();
    let mut rejected_rows = Vec::new();
    let mut defaulted_currency = false;
    let mut unclassified_count = 0_usize;
    let mut first_date = None::<String>;
    let mut last_date = None::<String>;

    for (record_index, record) in records.iter().enumerate().skip(header_index + 1) {
        if record.iter().all(|field| field.trim().is_empty()) {
            continue;
        }
        let row_number = record_index + 1;
        let Some(date) = parse_date(columns.required_field(record, columns.date)) else {
            record_rejection(&mut rejected_rows, row_number, "DATE");
            continue;
        };
        let currency = match columns.field(record, columns.currency) {
            Some(value) if !value.trim().is_empty() => match parse_currency(value) {
                Ok(currency) => currency,
                Err(_) => {
                    record_rejection(&mut rejected_rows, row_number, "CURRENCY");
                    continue;
                }
            },
            _ => {
                defaulted_currency = true;
                usd
            }
        };
        let money_decimals = currency.minor_unit_digits();
        let amount = columns
            .field(record, columns.amount)
            .and_then(|value| parse_scaled(Some(value), money_decimals));
        let quantity = columns
            .field(record, columns.quantity)
            .and_then(|value| parse_scaled(Some(value), 6))
            .map(PositionQuantity::from_scaled_units);
        let action = columns.field(record, columns.action).unwrap_or_default();
        let category = columns.field(record, columns.category).unwrap_or_default();
        let description = columns
            .field(record, columns.description)
            .unwrap_or_default();
        let kind = classify_activity(action, category, description, amount, columns.cash_export);
        if amount.is_none() && !(kind == PortfolioActivityKind::Split && quantity.is_some()) {
            record_rejection(&mut rejected_rows, row_number, "AMOUNT");
            continue;
        }
        if kind == PortfolioActivityKind::Other {
            unclassified_count += 1;
        }
        let raw_account = columns
            .field(record, columns.account)
            .filter(|value| !value.is_empty())
            .unwrap_or("DEFAULT");
        let next_account = account_aliases.len() + 1;
        let account_id = account_aliases
            .entry(raw_account.to_owned())
            .or_insert_with(|| PortfolioAccountId::new(format!("ACCOUNT {next_account}")))
            .clone();
        let symbol = columns
            .field(record, columns.symbol)
            .and_then(normalize_symbol);
        let fees = columns
            .field(record, columns.fees)
            .and_then(|value| parse_scaled(Some(value), money_decimals))
            .map(i128::abs)
            .filter(|value| *value != 0)
            .map(|value| Money::from_minor_units(value, currency));
        let description = display_description(description, category, action, kind);

        first_date = Some(first_date.map_or_else(|| date.clone(), |value| value.min(date.clone())));
        last_date = Some(last_date.map_or_else(|| date.clone(), |value| value.max(date.clone())));
        entries.push(PortfolioActivityEntry {
            activity_id: format!("ACT-{row_number:06}"),
            account_id,
            date,
            kind,
            description,
            symbol,
            currency,
            quantity,
            cash_effect: amount.map(|value| Money::from_minor_units(value, currency)),
            fees,
        });
    }
    if !rejected_rows.is_empty() {
        return Err(PortfolioError::InvalidCsv(format!(
            "REFUSED PARTIAL ACTIVITY IMPORT · INVALID {}",
            rejected_rows.join(", ")
        )));
    }
    if entries.is_empty() {
        return Err(PortfolioError::InvalidCsv(
            "NO DATED ACTIVITY ROWS WERE FOUND".to_owned(),
        ));
    }

    entries.sort_by(|left, right| {
        right
            .date
            .cmp(&left.date)
            .then_with(|| left.activity_id.cmp(&right.activity_id))
    });
    let currency_totals = reconcile_activity(&entries)?;
    let mut disclosures = vec![
        "PROVIDER CASH SIGN PRESERVED · POSITIVE IN / NEGATIVE OUT".to_owned(),
        "ACCOUNT IDENTIFIERS REPLACED WITH IMPORT-LOCAL LABELS".to_owned(),
        "ACTIVITY ALONE CANNOT PRODUCE TIME-WEIGHTED RETURN".to_owned(),
    ];
    if columns.cash_export {
        disclosures.push(
            "CASH-ACCOUNT ACTIVITY · NOT VERIFIED BROKER TRADE OR HOLDING HISTORY".to_owned(),
        );
        disclosures.push("MONARCH FORMAT USES POSITIVE INCOME AND NEGATIVE EXPENSES".to_owned());
    }
    if defaulted_currency {
        disclosures.push("MISSING CURRENCY DEFAULTED TO USD".to_owned());
    }
    if currency_totals.len() > 1 {
        disclosures.push("NO FX CONVERSION · CASH RECONCILES BY CURRENCY".to_owned());
    }
    if unclassified_count > 0 {
        disclosures.push(format!(
            "{unclassified_count} ACTIVITY ROW(S) RETAINED AS OTHER"
        ));
    }

    Ok(PortfolioActivityLedger {
        entries,
        currency_totals,
        source: format!("CSV · {source_name}"),
        period: match (first_date, last_date) {
            (Some(first), Some(last)) if first == last => first,
            (Some(first), Some(last)) => format!("{first} — {last}"),
            _ => "—".to_owned(),
        },
        input_version: csv_input_version(bytes),
        methodology: "EXACT PROVIDER CASH AMOUNTS · PER-CURRENCY SUM · NO FX · NO RETURN INFERENCE"
            .to_owned(),
        disclosures,
    })
}

fn reconcile_activity(
    entries: &[PortfolioActivityEntry],
) -> Result<Vec<PortfolioActivityCurrencyTotal>, PortfolioError> {
    let mut totals = BTreeMap::<Currency, AggregateCurrency>::new();
    for entry in entries {
        let total = totals.entry(entry.currency).or_default();
        total.entries += 1;
        if let Some(cash_effect) = entry.cash_effect {
            let amount = cash_effect.minor_units();
            total.net_cash_effect_minor = checked_add(
                total.net_cash_effect_minor,
                amount,
                "NET CASH EFFECT OVERFLOW",
            )?;
            if amount >= 0 {
                total.inflows_minor =
                    checked_add(total.inflows_minor, amount, "ACTIVITY INFLOW OVERFLOW")?;
            } else {
                total.outflows_minor = checked_add(
                    total.outflows_minor,
                    amount.checked_abs().ok_or_else(|| {
                        PortfolioError::InvalidCsv("ACTIVITY OUTFLOW OVERFLOW".to_owned())
                    })?,
                    "ACTIVITY OUTFLOW OVERFLOW",
                )?;
            }
            if entry.kind == PortfolioActivityKind::Dividend {
                total.dividends_minor =
                    checked_add(total.dividends_minor, amount, "DIVIDEND TOTAL OVERFLOW")?;
            }
            if entry.kind == PortfolioActivityKind::Interest {
                total.interest_minor =
                    checked_add(total.interest_minor, amount, "INTEREST TOTAL OVERFLOW")?;
            }
        } else {
            total.non_cash_entries += 1;
        }
        let fee_minor = entry.fees.map(Money::minor_units).or_else(|| {
            (entry.kind == PortfolioActivityKind::Fee)
                .then(|| entry.cash_effect.map(Money::minor_units))
                .flatten()
                .and_then(i128::checked_abs)
        });
        if let Some(fee_minor) = fee_minor {
            total.fees_minor = checked_add(total.fees_minor, fee_minor, "FEE TOTAL OVERFLOW")?;
        }
    }
    Ok(totals
        .into_iter()
        .map(|(currency, total)| PortfolioActivityCurrencyTotal {
            currency,
            entries: total.entries,
            inflows: Money::from_minor_units(total.inflows_minor, currency),
            outflows: Money::from_minor_units(total.outflows_minor, currency),
            net_cash_effect: Money::from_minor_units(total.net_cash_effect_minor, currency),
            dividends: Money::from_minor_units(total.dividends_minor, currency),
            interest: Money::from_minor_units(total.interest_minor, currency),
            fees: Money::from_minor_units(total.fees_minor, currency),
            non_cash_entries: total.non_cash_entries,
        })
        .collect())
}

fn checked_add(left: i128, right: i128, message: &str) -> Result<i128, PortfolioError> {
    left.checked_add(right)
        .ok_or_else(|| PortfolioError::InvalidCsv(message.to_owned()))
}

impl ActivityColumns {
    fn from_header(header: &StringRecord) -> Option<Self> {
        let names = header.iter().map(normalize_header).collect::<Vec<_>>();
        let date = find_preferred(
            &names,
            &[
                "tradedate",
                "transactiondate",
                "activitydate",
                "date",
                "settlementdate",
            ],
        )?;
        let amount = find_preferred(
            &names,
            &[
                "netamount",
                "cashamount",
                "amount",
                "principalamount",
                "totalamount",
            ],
        );
        let action = find_preferred(
            &names,
            &["transactiontype", "activitytype", "action", "type"],
        );
        let symbol = find_preferred(
            &names,
            &["symbol", "ticker", "securitysymbol", "investmentname"],
        );
        let quantity = find_preferred(&names, &["shares", "quantity", "qty", "units"]);
        if amount.is_none() && !(action.is_some() && symbol.is_some() && quantity.is_some()) {
            return None;
        }
        let category = find_preferred(&names, &["category", "transactioncategory"]);
        let merchant = find_preferred(&names, &["merchant"]);
        let description = merchant.or_else(|| {
            find_preferred(
                &names,
                &[
                    "description",
                    "securitydescription",
                    "originalstatement",
                    "memo",
                    "name",
                ],
            )
        });
        Some(Self {
            date,
            account: find_preferred(
                &names,
                &[
                    "account",
                    "accountname",
                    "accountnumber",
                    "accountid",
                    "portfolio",
                ],
            ),
            description,
            category,
            action,
            symbol,
            quantity,
            amount,
            fees: find_preferred(
                &names,
                &[
                    "commissionsandfees",
                    "commissionandfees",
                    "commission",
                    "fees",
                    "fee",
                ],
            ),
            currency: find_preferred(&names, &["currency", "ccy"]),
            cash_export: merchant.is_some() && category.is_some() && symbol.is_none(),
        })
    }

    fn field<'a>(&self, record: &'a StringRecord, column: Option<usize>) -> Option<&'a str> {
        column.and_then(|index| record.get(index)).map(str::trim)
    }

    fn required_field<'a>(&self, record: &'a StringRecord, column: usize) -> &'a str {
        record.get(column).map(str::trim).unwrap_or_default()
    }
}

fn classify_activity(
    action: &str,
    category: &str,
    description: &str,
    amount: Option<i128>,
    cash_export: bool,
) -> PortfolioActivityKind {
    let descriptor = normalize_words(&format!("{action} {category}"));
    let broader = if cash_export {
        descriptor.clone()
    } else {
        normalize_words(&format!("{action} {category} {description}"))
    };
    if broader.contains("split") {
        PortfolioActivityKind::Split
    } else if broader.contains("reinvest") {
        PortfolioActivityKind::Reinvestment
    } else if broader.contains("dividend") || broader.contains("capital gain distribution") {
        PortfolioActivityKind::Dividend
    } else if broader.contains("interest") {
        PortfolioActivityKind::Interest
    } else if broader.contains("commission") || broader.contains("fee") {
        PortfolioActivityKind::Fee
    } else if broader.contains("transfer") || broader.contains("credit card payment") {
        PortfolioActivityKind::Transfer
    } else if !cash_export && contains_word(&broader, "buy") {
        PortfolioActivityKind::Buy
    } else if !cash_export && contains_word(&broader, "sell") {
        PortfolioActivityKind::Sell
    } else if descriptor.contains("deposit")
        || descriptor.contains("income")
        || descriptor.contains("paycheck")
        || descriptor.contains("credit")
    {
        PortfolioActivityKind::CashIn
    } else if descriptor.contains("withdraw")
        || descriptor.contains("expense")
        || descriptor.contains("debit")
        || descriptor.contains("payment")
    {
        PortfolioActivityKind::CashOut
    } else {
        match amount {
            Some(value) if value > 0 => PortfolioActivityKind::CashIn,
            Some(value) if value < 0 => PortfolioActivityKind::CashOut,
            _ => PortfolioActivityKind::Other,
        }
    }
}

fn parse_date(value: &str) -> Option<String> {
    ["%Y-%m-%d", "%m/%d/%Y", "%m/%d/%y", "%Y/%m/%d"]
        .into_iter()
        .find_map(|format| NaiveDate::parse_from_str(value.trim(), format).ok())
        .map(|date| date.format("%Y-%m-%d").to_string())
}

fn display_description(
    description: &str,
    category: &str,
    action: &str,
    kind: PortfolioActivityKind,
) -> String {
    let value = [description, category, action]
        .into_iter()
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| kind.label());
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(96).collect()
}

fn normalize_symbol(value: &str) -> Option<String> {
    let value = value
        .trim()
        .trim_matches(|character| character == '*' || character == '"');
    (!value.is_empty()
        && value.len() <= 32
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '/' | '^' | '_')
        }))
    .then(|| value.to_ascii_uppercase())
}

fn record_rejection(rejections: &mut Vec<String>, row: usize, field: &str) {
    if rejections.len() < 8 {
        rejections.push(format!("ROW {row} {field}"));
    }
}

fn find_preferred(headers: &[String], aliases: &[&str]) -> Option<usize> {
    aliases
        .iter()
        .find_map(|alias| headers.iter().position(|header| header == alias))
}

fn normalize_header(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalize_words(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn contains_word(value: &str, word: &str) -> bool {
    value.split_whitespace().any(|candidate| candidate == word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_activity_reconciles_multicurrency_dividends_fees_and_split() {
        let csv = br#"Trade Date,Transaction Type,Description,Symbol,Shares,Net Amount,Commissions and Fees,Currency,Account
2026-01-02,Buy,BUY ACME,ACME,2,-200.00,1.00,USD,111
2026-02-03,Dividend,CASH DIVIDEND,ACME,,5.25,,USD,111
2026-03-04,Stock Split,2 FOR 1 SPLIT,ACME,2,,,USD,111
2026-04-05,Sell,SELL SAP,SAP,-1,125.50,0.50,EUR,222
"#;

        let ledger = parse_portfolio_activity_csv(csv, "activity.csv".to_owned()).unwrap();

        assert_eq!(ledger.entries.len(), 4);
        assert_eq!(ledger.period, "2026-01-02 — 2026-04-05");
        assert_eq!(ledger.currency_totals.len(), 2);
        let usd = ledger
            .currency_totals
            .iter()
            .find(|total| total.currency.as_str() == "USD")
            .unwrap();
        assert_eq!(usd.inflows.minor_units(), 525);
        assert_eq!(usd.outflows.minor_units(), 20_000);
        assert_eq!(usd.net_cash_effect.minor_units(), -19_475);
        assert_eq!(usd.dividends.minor_units(), 525);
        assert_eq!(usd.fees.minor_units(), 100);
        assert_eq!(usd.non_cash_entries, 1);
        assert!(ledger
            .entries
            .iter()
            .all(|entry| { !matches!(entry.account_id.as_str(), "111" | "222") }));
        assert!(ledger
            .disclosures
            .iter()
            .any(|value| value.contains("NO FX CONVERSION")));
    }

    #[test]
    fn monarch_cash_export_preserves_sign_and_stays_distinct_from_trade_history() {
        let csv = br#"Date,Merchant,Category,Account,Original Statement,Notes,Amount,Tags
08/01/2026,Employer,Paychecks,Checking,,,1000.25,
08/02/2026,Market,Groceries,Credit Card,,,-25.10,
08/03/2026,Move,Transfer,Checking,,,50.00,
"#;

        let ledger = parse_portfolio_activity_csv(csv, "cash.csv".to_owned()).unwrap();

        let usd = &ledger.currency_totals[0];
        assert_eq!(usd.inflows.minor_units(), 105_025);
        assert_eq!(usd.outflows.minor_units(), 2_510);
        assert_eq!(usd.net_cash_effect.minor_units(), 102_515);
        assert_eq!(ledger.entries[0].kind, PortfolioActivityKind::Transfer);
        assert!(ledger
            .disclosures
            .iter()
            .any(|value| value.contains("NOT VERIFIED BROKER TRADE")));
        assert!(ledger
            .disclosures
            .iter()
            .any(|value| value.contains("DEFAULTED TO USD")));
    }

    #[test]
    fn activity_import_fails_closed_instead_of_dropping_bad_rows() {
        let csv = b"Date,Description,Amount\n2026-01-01,Good,10.00\nnot-a-date,Bad,5.00\n";

        let error = parse_portfolio_activity_csv(csv, "bad.csv".to_owned()).unwrap_err();

        assert!(error
            .to_string()
            .contains("REFUSED PARTIAL ACTIVITY IMPORT"));
        assert!(error.to_string().contains("ROW 3 DATE"));
    }
}
