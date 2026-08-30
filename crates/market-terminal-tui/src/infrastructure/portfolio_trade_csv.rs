use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use csv::StringRecord;

use crate::{
    features::portfolio::{
        ExecutionPrice, PortfolioAccountId, PortfolioError, PortfolioTradeCurrencyTotal,
        PortfolioTradeExecution, PortfolioTradeLedger, PositionQuantity, TradeSide,
    },
    foundation::{Currency, InstrumentId, Money},
};

use super::portfolio_csv::{csv_input_version, parse_currency, parse_scaled};

pub(super) const MAX_TRADE_ROWS: usize = 100_000;
pub(super) const MAX_TRADE_COLUMNS: usize = 128;
const EXECUTION_SCALE: i128 = 1_000_000;

#[derive(Debug)]
struct TradeColumns {
    account: Option<usize>,
    order_id: Option<usize>,
    executed_at: usize,
    side: usize,
    symbol: usize,
    quantity: usize,
    price: usize,
    gross_amount: Option<usize>,
    commission: Option<usize>,
    fees: Option<usize>,
    net_amount: Option<usize>,
    currency: Option<usize>,
}

#[derive(Debug, Default)]
struct AggregateCurrency {
    fills: usize,
    buy_fills: usize,
    sell_fills: usize,
    buy_gross_minor: i128,
    sell_gross_minor: i128,
    fees_minor: i128,
    net_cash_effect_minor: i128,
}

pub(super) fn parse_portfolio_trade_csv(
    bytes: &[u8],
    source_name: String,
) -> Result<PortfolioTradeLedger, PortfolioError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);
    let records = reader
        .records()
        .take(MAX_TRADE_ROWS + 33)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| PortfolioError::InvalidCsv(format!("CSV PARSE ERROR · {error}")))?;
    if records
        .iter()
        .any(|record| record.len() > MAX_TRADE_COLUMNS)
    {
        return Err(PortfolioError::InvalidCsv(format!(
            "TRADE CSV EXCEEDS {MAX_TRADE_COLUMNS} COLUMNS"
        )));
    }
    let (header_index, columns) = records
        .iter()
        .take(32)
        .enumerate()
        .find_map(|(index, record)| {
            TradeColumns::from_header(record).map(|columns| (index, columns))
        })
        .ok_or_else(|| {
            PortfolioError::InvalidCsv(
                "NO TRADE HEADER FOUND · NEED EXECUTED AT, SIDE, SYMBOL, QUANTITY, AND PRICE"
                    .to_owned(),
            )
        })?;
    if records.len().saturating_sub(header_index + 1) > MAX_TRADE_ROWS {
        return Err(PortfolioError::InvalidCsv(format!(
            "TRADE CSV EXCEEDS {MAX_TRADE_ROWS} DATA ROWS"
        )));
    }

    let usd = Currency::new("USD").expect("USD is a valid currency");
    let mut executions = Vec::new();
    let mut account_aliases = BTreeMap::<String, PortfolioAccountId>::new();
    let mut order_aliases = BTreeMap::<String, String>::new();
    let mut rejected = Vec::new();
    let mut defaulted_currency = false;
    let mut imprecise_timestamps = 0_usize;
    let mut derived_gross = 0_usize;
    let mut derived_net = 0_usize;

    for (record_index, record) in records.iter().enumerate().skip(header_index + 1) {
        if record.iter().all(|field| field.trim().is_empty()) {
            continue;
        }
        let row = record_index + 1;
        let Some((executed_at, precise_timestamp)) =
            parse_timestamp(record.get(columns.executed_at).unwrap_or_default())
        else {
            reject(&mut rejected, row, "EXECUTED AT");
            continue;
        };
        if !precise_timestamp {
            imprecise_timestamps += 1;
        }
        let Some(side) = parse_side(record.get(columns.side).unwrap_or_default()) else {
            reject(&mut rejected, row, "SIDE");
            continue;
        };
        let Some(symbol) = normalize_symbol(record.get(columns.symbol).unwrap_or_default()) else {
            reject(&mut rejected, row, "SYMBOL");
            continue;
        };
        let currency = match columns.field(record, columns.currency) {
            Some(value) if !value.trim().is_empty() => match parse_currency(value) {
                Ok(currency) => currency,
                Err(_) => {
                    reject(&mut rejected, row, "CURRENCY");
                    continue;
                }
            },
            _ => {
                defaulted_currency = true;
                usd
            }
        };
        let Some(quantity) = parse_scaled(record.get(columns.quantity), 6) else {
            reject(&mut rejected, row, "QUANTITY");
            continue;
        };
        if quantity <= 0 {
            reject(&mut rejected, row, "NON-POSITIVE QUANTITY");
            continue;
        }
        let Some(price) = parse_scaled(record.get(columns.price), 6) else {
            reject(&mut rejected, row, "PRICE");
            continue;
        };
        if price <= 0 {
            reject(&mut rejected, row, "NON-POSITIVE PRICE");
            continue;
        }
        let decimals = currency.minor_unit_digits();
        let calculated_gross = calculate_gross(quantity, price, decimals)?;
        let gross_amount = match columns.field(record, columns.gross_amount) {
            Some(value) if !value.trim().is_empty() => {
                let Some(value) = parse_scaled(Some(value), decimals) else {
                    reject(&mut rejected, row, "GROSS AMOUNT");
                    continue;
                };
                let Some(value) = value.checked_abs() else {
                    return Err(PortfolioError::InvalidCsv(
                        "TRADE GROSS AMOUNT OVERFLOW".to_owned(),
                    ));
                };
                if value != calculated_gross {
                    reject(&mut rejected, row, "GROSS DOES NOT MATCH QUANTITY × PRICE");
                    continue;
                }
                value
            }
            _ => {
                derived_gross += 1;
                calculated_gross
            }
        };
        let commission = match parse_optional_money(record, columns.commission, decimals) {
            Ok(value) => value,
            Err(()) => {
                reject(&mut rejected, row, "COMMISSION");
                continue;
            }
        };
        let other_fees = match parse_optional_money(record, columns.fees, decimals) {
            Ok(value) => value,
            Err(()) => {
                reject(&mut rejected, row, "FEES");
                continue;
            }
        };
        let Some(fees) = commission.checked_add(other_fees) else {
            return Err(PortfolioError::InvalidCsv("TRADE FEE OVERFLOW".to_owned()));
        };
        let expected_net = match side {
            TradeSide::Buy => gross_amount.checked_add(fees).and_then(i128::checked_neg),
            TradeSide::Sell => gross_amount.checked_sub(fees),
        }
        .ok_or_else(|| PortfolioError::InvalidCsv("TRADE NET CASH OVERFLOW".to_owned()))?;
        let net_cash_effect = match columns.field(record, columns.net_amount) {
            Some(value) if !value.trim().is_empty() => {
                let Some(value) = parse_scaled(Some(value), decimals) else {
                    reject(&mut rejected, row, "NET AMOUNT");
                    continue;
                };
                if value != expected_net {
                    reject(&mut rejected, row, "NET AMOUNT DOES NOT RECONCILE");
                    continue;
                }
                value
            }
            _ => {
                derived_net += 1;
                expected_net
            }
        };
        let raw_account = columns
            .field(record, columns.account)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("DEFAULT");
        let next_account = account_aliases.len() + 1;
        let account_id = account_aliases
            .entry(raw_account.to_owned())
            .or_insert_with(|| PortfolioAccountId::new(format!("ACCOUNT {next_account}")))
            .clone();
        let raw_order = columns
            .field(record, columns.order_id)
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("ROW-{row}"));
        let next_order = order_aliases.len() + 1;
        let order_id = order_aliases
            .entry(raw_order)
            .or_insert_with(|| format!("ORDER {next_order}"))
            .clone();

        executions.push(PortfolioTradeExecution {
            execution_id: format!("FILL-{row:06}"),
            order_id,
            account_id,
            instrument_id: InstrumentId::new(format!(
                "unresolved:portfolio:{}",
                symbol.to_ascii_lowercase()
            )),
            symbol,
            executed_at,
            side,
            currency,
            quantity: PositionQuantity::from_scaled_units(quantity),
            fill_price: ExecutionPrice::from_scaled_units(price),
            gross_amount: Money::from_minor_units(gross_amount, currency),
            fees: Money::from_minor_units(fees, currency),
            net_cash_effect: Money::from_minor_units(net_cash_effect, currency),
        });
    }
    if !rejected.is_empty() {
        return Err(PortfolioError::InvalidCsv(format!(
            "REFUSED PARTIAL TRADE IMPORT · INVALID {}",
            rejected.join(", ")
        )));
    }
    if executions.is_empty() {
        return Err(PortfolioError::InvalidCsv(
            "NO BROKER EXECUTIONS WERE FOUND".to_owned(),
        ));
    }

    executions.sort_by(|left, right| {
        right
            .executed_at
            .cmp(&left.executed_at)
            .then_with(|| left.order_id.cmp(&right.order_id))
            .then_with(|| left.execution_id.cmp(&right.execution_id))
    });
    let first_date = executions
        .iter()
        .map(|execution| &execution.executed_at[..10])
        .min()
        .expect("non-empty executions");
    let last_date = executions
        .iter()
        .map(|execution| &execution.executed_at[..10])
        .max()
        .expect("non-empty executions");
    let period = if first_date == last_date {
        first_date.to_owned()
    } else {
        format!("{first_date} — {last_date}")
    };
    let currency_totals = reconcile(&executions)?;
    let mut disclosures = vec![
        "READ-ONLY BROKER EXECUTIONS · NO ORDER ROUTING OR SUBMISSION".to_owned(),
        "BUY NET CASH IS −(GROSS + FEES); SELL NET CASH IS GROSS − FEES".to_owned(),
        "BROKER ACCOUNT AND ORDER IDENTIFIERS REPLACED WITH IMPORT-LOCAL LABELS".to_owned(),
        "CASH ACTIVITY IS NOT PROMOTED INTO EXECUTION EVIDENCE".to_owned(),
        "TICKERS REMAIN UNRESOLVED UNTIL INSTRUMENT-MASTER MATCHING".to_owned(),
    ];
    if defaulted_currency {
        disclosures.push("MISSING CURRENCY DEFAULTED TO USD".to_owned());
    }
    if imprecise_timestamps > 0 {
        disclosures.push(format!(
            "{imprecise_timestamps} FILL(S) LACK A PROVIDER UTC TIMESTAMP"
        ));
    }
    if derived_gross > 0 {
        disclosures.push(format!(
            "{derived_gross} GROSS AMOUNT(S) DERIVED FROM QUANTITY × SIX-DECIMAL PRICE"
        ));
    }
    if derived_net > 0 {
        disclosures.push(format!(
            "{derived_net} NET CASH EFFECT(S) DERIVED FROM SIDE, GROSS, AND FEES"
        ));
    }
    if currency_totals.len() > 1 {
        disclosures.push("NO FX CONVERSION · EXECUTIONS RECONCILE BY CURRENCY".to_owned());
    }

    Ok(PortfolioTradeLedger {
        executions,
        currency_totals,
        source: format!("CSV · {source_name}"),
        period,
        input_version: csv_input_version(bytes),
        methodology: "BROKER EXECUTIONS · EXACT QUANTITY/PRICE/GROSS/FEE/NET CHECKS · NO FX"
            .to_owned(),
        disclosures,
    })
}

fn reconcile(
    executions: &[PortfolioTradeExecution],
) -> Result<Vec<PortfolioTradeCurrencyTotal>, PortfolioError> {
    let mut totals = BTreeMap::<Currency, AggregateCurrency>::new();
    for execution in executions {
        let total = totals.entry(execution.currency).or_default();
        total.fills += 1;
        match execution.side {
            TradeSide::Buy => {
                total.buy_fills += 1;
                total.buy_gross_minor =
                    checked_add(total.buy_gross_minor, execution.gross_amount.minor_units())?;
            }
            TradeSide::Sell => {
                total.sell_fills += 1;
                total.sell_gross_minor =
                    checked_add(total.sell_gross_minor, execution.gross_amount.minor_units())?;
            }
        }
        total.fees_minor = checked_add(total.fees_minor, execution.fees.minor_units())?;
        total.net_cash_effect_minor = checked_add(
            total.net_cash_effect_minor,
            execution.net_cash_effect.minor_units(),
        )?;
    }
    Ok(totals
        .into_iter()
        .map(|(currency, total)| PortfolioTradeCurrencyTotal {
            currency,
            fills: total.fills,
            buy_fills: total.buy_fills,
            sell_fills: total.sell_fills,
            buy_gross: Money::from_minor_units(total.buy_gross_minor, currency),
            sell_gross: Money::from_minor_units(total.sell_gross_minor, currency),
            fees: Money::from_minor_units(total.fees_minor, currency),
            net_cash_effect: Money::from_minor_units(total.net_cash_effect_minor, currency),
        })
        .collect())
}

fn calculate_gross(
    quantity: i128,
    price: i128,
    money_decimals: u32,
) -> Result<i128, PortfolioError> {
    let numerator = quantity
        .checked_mul(price)
        .and_then(|value| value.checked_mul(10_i128.pow(money_decimals)))
        .ok_or_else(|| PortfolioError::InvalidCsv("TRADE GROSS OVERFLOW".to_owned()))?;
    Ok(rounded_division(
        numerator,
        EXECUTION_SCALE * EXECUTION_SCALE,
    ))
}

fn rounded_division(numerator: i128, denominator: i128) -> i128 {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder.unsigned_abs().saturating_mul(2) >= denominator.unsigned_abs() {
        quotient + numerator.signum()
    } else {
        quotient
    }
}

fn checked_add(left: i128, right: i128) -> Result<i128, PortfolioError> {
    left.checked_add(right)
        .ok_or_else(|| PortfolioError::InvalidCsv("TRADE TOTAL OVERFLOW".to_owned()))
}

fn parse_optional_money(
    record: &StringRecord,
    column: Option<usize>,
    decimals: u32,
) -> Result<i128, ()> {
    let Some(value) = column.and_then(|column| record.get(column)) else {
        return Ok(0);
    };
    if value.trim().is_empty() {
        return Ok(0);
    }
    parse_scaled(Some(value), decimals)
        .and_then(i128::checked_abs)
        .ok_or(())
}

fn parse_timestamp(value: &str) -> Option<(String, bool)> {
    let value = value.trim();
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(value) {
        return Some((
            timestamp
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::AutoSi, true),
            true,
        ));
    }
    for format in ["%Y-%m-%d %H:%M:%S", "%m/%d/%Y %H:%M:%S"] {
        if let Ok(timestamp) = NaiveDateTime::parse_from_str(value, format) {
            return Some((
                format!("{} · TZ UNSPECIFIED", timestamp.format("%Y-%m-%d %H:%M:%S")),
                false,
            ));
        }
    }
    for format in ["%Y-%m-%d", "%m/%d/%Y", "%m/%d/%y"] {
        if let Ok(date) = NaiveDate::parse_from_str(value, format) {
            return Some((
                format!("{} · TIME UNSPECIFIED", date.format("%Y-%m-%d")),
                false,
            ));
        }
    }
    None
}

fn parse_side(value: &str) -> Option<TradeSide> {
    match normalize_header(value).as_str() {
        "buy" | "bought" | "youbought" | "b" | "buytoopen" | "buytoclose" => Some(TradeSide::Buy),
        "sell" | "sold" | "yousold" | "s" | "selltoopen" | "selltoclose" => Some(TradeSide::Sell),
        _ => None,
    }
}

fn normalize_symbol(value: &str) -> Option<String> {
    let value = value.trim().trim_start_matches('$').to_ascii_uppercase();
    let value = value.replace('/', ".");
    (!value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')))
    .then_some(value)
}

fn reject(rejected: &mut Vec<String>, row: usize, field: &str) {
    if rejected.len() < 8 {
        rejected.push(format!("ROW {row} {field}"));
    }
}

impl TradeColumns {
    fn from_header(header: &StringRecord) -> Option<Self> {
        let names = header.iter().map(normalize_header).collect::<Vec<_>>();
        Some(Self {
            account: find(&names, &["account", "accountname", "accountnumber"]),
            order_id: find(&names, &["orderid", "ordernumber", "order", "tradeid"]),
            executed_at: find(
                &names,
                &[
                    "executedat",
                    "executiontime",
                    "executedtime",
                    "filltime",
                    "filledat",
                    "tradedatetime",
                    "datetime",
                    "dateandtime",
                    "tradedate",
                    "date",
                ],
            )?,
            side: find(&names, &["side", "buysell", "action", "transactiontype"])?,
            symbol: find(
                &names,
                &["symbol", "ticker", "tickersymbol", "securitysymbol"],
            )?,
            quantity: find(
                &names,
                &[
                    "quantity",
                    "filledquantity",
                    "fillquantity",
                    "qty",
                    "shares",
                ],
            )?,
            price: find(
                &names,
                &[
                    "price",
                    "fillprice",
                    "executionprice",
                    "averageprice",
                    "tprice",
                ],
            )?,
            gross_amount: find(
                &names,
                &["grossamount", "principalamount", "principal", "tradevalue"],
            ),
            commission: find(
                &names,
                &[
                    "commission",
                    "commissions",
                    "commissionandfees",
                    "commissionsandfees",
                ],
            ),
            fees: find(
                &names,
                &["fees", "fee", "transactionfees", "regulatoryfees"],
            ),
            net_amount: find(&names, &["netamount", "netcash", "netcasheffect"]),
            currency: find(&names, &["currency", "currencycode", "ccy"]),
        })
    }

    fn field<'a>(&self, record: &'a StringRecord, column: Option<usize>) -> Option<&'a str> {
        column.and_then(|column| record.get(column))
    }
}

fn normalize_header(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn find(names: &[String], candidates: &[&str]) -> Option<usize> {
    candidates
        .iter()
        .find_map(|candidate| names.iter().position(|name| name == candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconciles_verified_buys_sells_fees_and_orders() {
        let csv = b"Account,Order ID,Executed At,Side,Symbol,Quantity,Price,Gross Amount,Commission,Fees,Net Amount,Currency\nBroker 123,abc,2026-08-01T14:30:00-04:00,Buy,AAPL,2.5,200.00,500.00,1.00,0.25,-501.25,USD\nBroker 123,def,2026-08-02T15:00:00Z,Sell,AAPL,1,220.00,220.00,1.00,0.10,218.90,USD\nBroker 456,ghi,2026-08-03T10:00:00Z,Sell,SAP,2,100.00,200.00,2.00,0,198.00,EUR\n";

        let ledger = parse_portfolio_trade_csv(csv, "trades.csv".to_owned()).unwrap();

        assert_eq!(ledger.executions.len(), 3);
        assert_eq!(ledger.currency_totals.len(), 2);
        let usd = ledger
            .currency_totals
            .iter()
            .find(|total| total.currency.as_str() == "USD")
            .unwrap();
        assert_eq!(usd.buy_gross.minor_units(), 50_000);
        assert_eq!(usd.sell_gross.minor_units(), 22_000);
        assert_eq!(usd.fees.minor_units(), 235);
        assert_eq!(usd.net_cash_effect.minor_units(), -28_235);
        assert_eq!(ledger.executions[1].fill_price.scaled_units(), 220_000_000);
        assert!(ledger.executions.iter().all(|execution| {
            execution.account_id.as_str().starts_with("ACCOUNT ")
                && execution.order_id.starts_with("ORDER ")
        }));
        assert!(ledger
            .disclosures
            .iter()
            .any(|value| value.contains("NO FX CONVERSION")));
    }

    #[test]
    fn derives_missing_gross_and_net_with_exact_fractional_math() {
        let csv = b"Trade Date,Action,Symbol,Shares,Price,Commission\n08/01/2026,Buy,BRK/B,0.125,400.123456,0.01\n";

        let ledger = parse_portfolio_trade_csv(csv, "fills.csv".to_owned()).unwrap();

        assert_eq!(ledger.executions[0].symbol, "BRK.B");
        assert_eq!(ledger.executions[0].gross_amount.minor_units(), 5_002);
        assert_eq!(ledger.executions[0].net_cash_effect.minor_units(), -5_003);
        assert!(ledger
            .disclosures
            .iter()
            .any(|value| value.contains("GROSS AMOUNT(S) DERIVED")));
        assert!(ledger
            .disclosures
            .iter()
            .any(|value| value.contains("LACK A PROVIDER UTC TIMESTAMP")));
    }

    #[test]
    fn refuses_partial_imports_and_cash_mismatches() {
        let bad_net = b"Executed At,Side,Symbol,Quantity,Price,Net Amount\n2026-08-01T10:00:00Z,Buy,AAPL,1,100,-99\n";
        let bad_side =
            b"Executed At,Side,Symbol,Quantity,Price\n2026-08-01T10:00:00Z,HOLD,AAPL,1,100\n";

        assert!(parse_portfolio_trade_csv(bad_net, "bad.csv".to_owned())
            .unwrap_err()
            .to_string()
            .contains("NET AMOUNT DOES NOT RECONCILE"));
        assert!(parse_portfolio_trade_csv(bad_side, "bad.csv".to_owned())
            .unwrap_err()
            .to_string()
            .contains("ROW 2 SIDE"));
    }
}
