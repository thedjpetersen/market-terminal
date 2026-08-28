use crate::features::{
    markets::{MarketIndex, MarketsQuery, MarketsSnapshot},
    news::{Headline, NewsFeed, NewsSnapshot},
    overview::{OverviewQuery, OverviewSnapshot},
    portfolio::{
        PortfolioAccountId, PortfolioClosedLot, PortfolioCurrencyTotal, PortfolioPerformanceSeries,
        PortfolioPerformanceSnapshot, PortfolioRealizedGainCurrencyTotal,
        PortfolioRealizedGainSnapshot, PortfolioRepository, PortfolioSnapshot, PortfolioTaxLot,
        PortfolioTaxLotCurrencyTotal, PortfolioTaxLotSnapshot, PortfolioValuationPoint, Position,
        PositionQuantity, TaxLotHoldingPeriod,
    },
    security::{
        Estimate, Filing, FinancialPeriod, OwnerPosition, PeerComparison, SecurityError,
        SecurityIdentity, SecurityPage, SecurityQuery, SecurityResearch, SecuritySnapshot,
    },
};
use crate::foundation::{Currency, InstrumentId, Money};

#[derive(Debug, Default, Clone, Copy)]
pub struct DemoData;

const PERIODS: [&str; 8] = ["D", "M", "6M", "YTD", "1Y", "2Y", "5Y", "10Y"];
const RETURNS_A: [(f64, f64); 24] = [
    (0., 1.1),
    (1., 2.1),
    (5., 3.5),
    (10., 4.8),
    (12., 3.5),
    (18., 4.7),
    (25., 5.),
    (31., 3.3),
    (38., -0.1),
    (40., 1.),
    (45., 3.6),
    (47., 5.),
    (53., 7.7),
    (61., 9.),
    (65., 10.3),
    (70., 11.7),
    (73., 10.3),
    (79., 13.),
    (84., 14.4),
    (88., 11.7),
    (94., 14.4),
    (97., 15.8),
    (99., 14.5),
    (100., 17.1),
];
const RETURNS_B: [(f64, f64); 20] = [
    (0., 1.),
    (1., 2.2),
    (4., 3.5),
    (9., 2.1),
    (17., 3.5),
    (24., 2.2),
    (31., 2.1),
    (37., -1.7),
    (40., 0.9),
    (48., 5.),
    (50., 6.4),
    (62., 9.),
    (67., 10.4),
    (71., 9.),
    (75., 11.7),
    (82., 13.),
    (88., 10.3),
    (94., 13.),
    (97., 14.4),
    (100., 14.3),
];
const SECURITY_PRICE: [(f64, f64); 18] = [
    (0., 196.3),
    (5., 197.1),
    (10., 196.7),
    (16., 198.4),
    (21., 197.8),
    (27., 199.2),
    (33., 199.8),
    (39., 199.1),
    (45., 200.4),
    (52., 201.1),
    (58., 200.7),
    (64., 202.4),
    (70., 203.2),
    (76., 202.7),
    (82., 203.8),
    (88., 204.6),
    (94., 204.1),
    (100., 205.3),
];
const TREASURY_CURVE: [(f64, f64); 8] = [
    (0., 5.38),
    (14., 4.91),
    (28., 4.78),
    (43., 4.47),
    (57., 4.40),
    (72., 4.31),
    (86., 4.38),
    (100., 4.47),
];

const MARKETS: [MarketIndex; 7] = [
    MarketIndex {
        name: "S&P 500",
        symbol: "SPX",
        last: "5,304.72",
        net_change: "+45.18",
        percent_change: "+0.86%",
    },
    MarketIndex {
        name: "NASDAQ 100",
        symbol: "NDX",
        last: "18,658.32",
        net_change: "+183.72",
        percent_change: "+1.00%",
    },
    MarketIndex {
        name: "DOW JONES",
        symbol: "INDU",
        last: "39,512.88",
        net_change: "+305.18",
        percent_change: "+0.78%",
    },
    MarketIndex {
        name: "RUSSELL 2000",
        symbol: "RTY",
        last: "2,075.34",
        net_change: "+20.11",
        percent_change: "+0.98%",
    },
    MarketIndex {
        name: "STOXX 600",
        symbol: "SXXP",
        last: "514.22",
        net_change: "+2.04",
        percent_change: "+0.40%",
    },
    MarketIndex {
        name: "NIKKEI 225",
        symbol: "NKY",
        last: "38,487.90",
        net_change: "+1,724.25",
        percent_change: "+4.69%",
    },
    MarketIndex {
        name: "HANG SENG",
        symbol: "HSI",
        last: "18,028.57",
        net_change: "−150.58",
        percent_change: "−0.83%",
    },
];

const POSITIONS: [(&str, &str, &str, &str, &str, &str); 7] = [
    ("NVDA", "1,240", "155.04", "192,250", "+42.18%", "18.4%"),
    ("AAPL", "820", "164.21", "168,346", "+25.02%", "16.1%"),
    ("MSFT", "315", "382.80", "141,422", "+17.30%", "13.5%"),
    ("META", "228", "421.14", "116,967", "+21.82%", "11.2%"),
    ("AMZN", "540", "156.08", "100,391", "+19.12%", "9.6%"),
    ("VTI", "412", "236.44", "111,231", "+14.18%", "10.6%"),
    ("SGOV", "650", "100.31", "65,208", "+1.03%", "6.2%"),
];

const HEADLINES: [(&str, &str, &str, &str); 10] = [
    (
        "16:00",
        "TOP",
        "Wall Street closes higher as chipmakers lead broad rally",
        "US",
    ),
    (
        "15:45",
        "FED",
        "Fed's Barkin: inflation progress has resumed",
        "US",
    ),
    (
        "15:30",
        "ECO",
        "Treasury yields fall after softer housing data",
        "US",
    ),
    (
        "15:16",
        "CMD",
        "Oil prices slide on weaker demand outlook",
        "GL",
    ),
    (
        "14:58",
        "EQU",
        "European markets mixed ahead of U.S. close",
        "EU",
    ),
    (
        "14:41",
        "TEC",
        "Micron outlook revives AI trade; suppliers surge",
        "AS",
    ),
    (
        "14:22",
        "FX",
        "Dollar slips as traders bring forward first rate cut",
        "GL",
    ),
    (
        "13:55",
        "POL",
        "EU agrees strategic investment framework",
        "EU",
    ),
    (
        "13:31",
        "EQU",
        "Small caps outperform for third session",
        "US",
    ),
    (
        "12:48",
        "CMD",
        "Copper advances on China stimulus expectations",
        "AS",
    ),
];

impl OverviewQuery for DemoData {
    fn load_overview(&self) -> OverviewSnapshot {
        OverviewSnapshot::Gallery {
            periods: &PERIODS,
            primary_returns: &RETURNS_A,
            comparison_returns: &RETURNS_B,
        }
    }
}

impl MarketsQuery for DemoData {
    fn load_markets(&self) -> MarketsSnapshot {
        MarketsSnapshot::Gallery {
            indices: &MARKETS,
            treasury_curve: &TREASURY_CURVE,
        }
    }
}

impl SecurityQuery for DemoData {
    fn load_security(&self, symbol: &str) -> Result<SecurityPage, SecurityError> {
        let (symbol, name, last, absolute_change, percent_change) = match symbol
            .split_whitespace()
            .next()
            .unwrap_or("AAPL")
            .to_ascii_uppercase()
            .as_str()
        {
            "MSFT" => (
                "MSFT US EQUITY",
                "MICROSOFT CORP",
                "512.44",
                "+3.81",
                "+0.75%",
            ),
            "NVDA" => ("NVDA US EQUITY", "NVIDIA CORP", "184.92", "+4.26", "+2.36%"),
            "META" => (
                "META US EQUITY",
                "META PLATFORMS",
                "738.10",
                "+8.42",
                "+1.15%",
            ),
            "SPY" => (
                "SPY US ETF",
                "SPDR S&P 500 ETF",
                "653.28",
                "+4.13",
                "+0.64%",
            ),
            _ => ("AAPL US EQUITY", "APPLE INC", "205.30", "+1.72", "+0.84%"),
        };
        Ok(SecurityPage {
            snapshot: SecuritySnapshot {
                symbol: symbol.to_owned(),
                name: name.to_owned(),
                last: last.to_owned(),
                absolute_change: absolute_change.to_owned(),
                percent_change: percent_change.to_owned(),
                session_summary: "OPEN 203.41  HIGH 205.64  LOW 202.72  VOLUME 41.82M".to_owned(),
                price_series: SECURITY_PRICE.to_vec(),
                statistics: vec![
                    ("MARKET CAP".to_owned(), "$3.15T".to_owned()),
                    ("P/E (TTM)".to_owned(), "31.92X".to_owned()),
                    ("52W RANGE".to_owned(), "164—237".to_owned()),
                    ("DATA MODE".to_owned(), "REPLAY".to_owned()),
                ],
                source: "DETERMINISTIC DEMO".to_owned(),
            },
            research: demo_security_research(symbol),
        })
    }
}

fn demo_security_research(symbol: &str) -> SecurityResearch {
    SecurityResearch {
        identity: SecurityIdentity::from_terminal_symbol(symbol),
        financials: vec![FinancialPeriod {
            period: "FY24A".to_owned(),
            revenue_billions: "391.0".to_owned(),
            operating_income_billions: "123.2".to_owned(),
            net_income_billions: "93.7".to_owned(),
            diluted_eps: "6.57".to_owned(),
        }],
        estimates: vec![Estimate {
            period: "FY25E".to_owned(),
            revenue: "414.8B".to_owned(),
            eps: "7.24".to_owned(),
            eps_high: "7.48".to_owned(),
            eps_low: "6.98".to_owned(),
        }],
        owners: vec![OwnerPosition {
            manager: "DEMO MANAGER".to_owned(),
            shares: "1.0M".to_owned(),
            value: "$205.3M".to_owned(),
            quarterly_change: "+0.1%".to_owned(),
        }],
        insider_transactions: Vec::new(),
        insider_status: "GALLERY REPLAY · NO LIVE FORM 4 REQUEST".to_owned(),
        filings: vec![Filing {
            filed: "2025-11-01".to_owned(),
            form: "10-K".to_owned(),
            period: "2025-09-27".to_owned(),
            description: "DETERMINISTIC ANNUAL REPORT".to_owned(),
            accession: "DEMO-ACCESSION".to_owned(),
            document_url: None,
        }],
        peers: vec![PeerComparison {
            symbol: "DEMO".to_owned(),
            name: "DETERMINISTIC PEER".to_owned(),
            price_to_earnings: "29.4x".to_owned(),
            ev_to_ebitda: "22.8x".to_owned(),
            revenue_growth: "+6.1%".to_owned(),
            gross_margin: "46.3%".to_owned(),
        }],
        source: "DETERMINISTIC DEMO".to_owned(),
    }
}

impl PortfolioRepository for DemoData {
    fn load_portfolio(&self) -> PortfolioSnapshot {
        let usd = Currency::new("USD").expect("USD is valid");
        let mut positions = POSITIONS
            .iter()
            .map(|position| Position {
                instrument_id: InstrumentId::new(format!(
                    "demo:instrument:{}",
                    position.0.to_ascii_lowercase()
                )),
                account_id: PortfolioAccountId::new("DEMO ACCOUNT"),
                symbol: position.0.to_owned(),
                currency: usd,
                quantity: PositionQuantity::from_scaled_units(demo_decimal(position.1, 6)),
                average_cost: Some(Money::from_minor_units(demo_decimal(position.2, 2), usd)),
                market_value: Some(Money::from_minor_units(demo_decimal(position.3, 2), usd)),
                unrealized_return_bps: Some(demo_decimal(position.4, 2) as i32),
                weight_bps: Some(demo_decimal(position.5, 2) as i32),
                cash: false,
            })
            .collect::<Vec<_>>();
        positions.extend([
            Position {
                instrument_id: InstrumentId::new("cash:usd"),
                account_id: PortfolioAccountId::new("DEMO ACCOUNT"),
                symbol: "CASH".to_owned(),
                currency: usd,
                quantity: PositionQuantity::from_scaled_units(127_834_000_000),
                average_cost: Some(Money::from_minor_units(100, usd)),
                market_value: Some(Money::from_minor_units(12_783_400, usd)),
                unrealized_return_bps: Some(0),
                weight_bps: Some(1_223),
                cash: true,
            },
            Position {
                instrument_id: InstrumentId::new("demo:instrument:other"),
                account_id: PortfolioAccountId::new("DEMO ACCOUNT"),
                symbol: "OTHER".to_owned(),
                currency: usd,
                quantity: PositionQuantity::from_scaled_units(1_000_000),
                average_cost: None,
                market_value: Some(Money::from_minor_units(2_157_900, usd)),
                unrealized_return_bps: None,
                weight_bps: Some(206),
                cash: false,
            },
        ]);
        let priced_positions = positions.len();
        PortfolioSnapshot {
            positions,
            currency_totals: vec![PortfolioCurrencyTotal {
                currency: usd,
                net_asset_value: Money::from_minor_units(104_522_800, usd),
                available_cash: Money::from_minor_units(12_783_400, usd),
                priced_positions,
                unpriced_positions: 0,
            }],
            ytd_return_bps: Some(1_702),
            sharpe_hundredths: Some(279),
            source: "DETERMINISTIC DEMO".to_owned(),
            as_of: "2026-08-25 16:00 ET".to_owned(),
            input_version: "DEMO-V1".to_owned(),
            methodology: "DETERMINISTIC GALLERY FIXTURE".to_owned(),
            disclosures: vec!["NOT INTERACTIVE USER DATA".to_owned()],
        }
    }

    fn load_performance(&self) -> PortfolioPerformanceSnapshot {
        let usd = Currency::new("USD").expect("USD is valid");
        let point =
            |date: &str, value: i128, flow: i128, benchmark: i128| PortfolioValuationPoint {
                date: date.to_owned(),
                currency: usd,
                ending_value: Money::from_minor_units(value, usd),
                external_flow: Money::from_minor_units(flow, usd),
                benchmark_value: Some(Money::from_minor_units(benchmark, usd)),
            };
        PortfolioPerformanceSnapshot {
            series: vec![PortfolioPerformanceSeries {
                currency: usd,
                points: vec![
                    point("2026-01-02", 100_000_000, 0, 10_000),
                    point("2026-04-01", 108_000_000, 2_000_000, 10_400),
                    point("2026-07-01", 114_000_000, 0, 10_800),
                    point("2026-08-25", 121_000_000, 1_000_000, 11_240),
                ],
                time_weighted_return_bps: 1_778,
                benchmark_return_bps: Some(1_240),
                active_return_bps: Some(538),
            }],
            source: "DETERMINISTIC DEMO".to_owned(),
            period: "2026-01-02 — 2026-08-25".to_owned(),
            input_version: "DEMO-PERFORMANCE-V1".to_owned(),
            methodology: "END-OF-PERIOD FLOW-ADJUSTED TWR · USD".to_owned(),
            disclosures: vec![
                "DETERMINISTIC GALLERY VALUATIONS · NOT INTERACTIVE USER DATA".to_owned(),
                "BENCHMARK IS A DETERMINISTIC COMPARISON SERIES".to_owned(),
                "NO POSITION CONTRIBUTION OR FACTOR ATTRIBUTION".to_owned(),
            ],
        }
    }

    fn load_tax_lots(&self) -> PortfolioTaxLotSnapshot {
        let usd = Currency::new("USD").expect("USD is valid");
        let lot = |id: &str,
                   symbol: &str,
                   date: &str,
                   term: TaxLotHoldingPeriod,
                   quantity: i128,
                   basis: i128,
                   value: i128,
                   return_bps: i32| PortfolioTaxLot {
            lot_id: id.to_owned(),
            account_id: PortfolioAccountId::new("DEMO ACCOUNT"),
            instrument_id: InstrumentId::new(format!(
                "demo:instrument:{}",
                symbol.to_ascii_lowercase()
            )),
            symbol: symbol.to_owned(),
            acquired_date: date.to_owned(),
            holding_period: term,
            currency: usd,
            quantity: PositionQuantity::from_scaled_units(quantity),
            cost_basis: Money::from_minor_units(basis, usd),
            current_value: Some(Money::from_minor_units(value, usd)),
            unrealized_gain: Some(Money::from_minor_units(value - basis, usd)),
            unrealized_return_bps: Some(return_bps),
        };
        PortfolioTaxLotSnapshot {
            lots: vec![
                lot(
                    "DEMO-LOT-1",
                    "META",
                    "2024-01-12",
                    TaxLotHoldingPeriod::LongTerm,
                    100_000_000,
                    3_000_000,
                    5_000_000,
                    6_667,
                ),
                lot(
                    "DEMO-LOT-2",
                    "META",
                    "2026-05-15",
                    TaxLotHoldingPeriod::ShortTerm,
                    50_000_000,
                    2_000_000,
                    2_500_000,
                    2_500,
                ),
                lot(
                    "DEMO-LOT-3",
                    "AAPL",
                    "2023-09-08",
                    TaxLotHoldingPeriod::LongTerm,
                    100_000_000,
                    1_500_000,
                    2_000_000,
                    3_333,
                ),
            ],
            currency_totals: vec![PortfolioTaxLotCurrencyTotal {
                currency: usd,
                lots: 3,
                cost_basis: Money::from_minor_units(6_500_000, usd),
                priced_cost_basis: Money::from_minor_units(6_500_000, usd),
                current_value: Money::from_minor_units(9_500_000, usd),
                unrealized_gain: Money::from_minor_units(3_000_000, usd),
                unpriced_lots: 0,
            }],
            source: "DETERMINISTIC DEMO".to_owned(),
            as_of: "2026-08-25 16:00 ET".to_owned(),
            input_version: "DEMO-TAX-LOTS-V1".to_owned(),
            methodology: "DETERMINISTIC OPEN-LOT BASIS FIXTURE · USD".to_owned(),
            disclosures: vec![
                "DETERMINISTIC GALLERY LOTS · NOT INTERACTIVE USER DATA".to_owned(),
                "OPEN LOTS ONLY · NO REALIZED-GAIN OR CLOSED-TRADE HISTORY".to_owned(),
            ],
        }
    }

    fn load_realized_gains(&self) -> PortfolioRealizedGainSnapshot {
        let usd = Currency::new("USD").expect("USD is valid");
        let lot = |id: &str,
                   symbol: &str,
                   acquired: &str,
                   disposed: &str,
                   term: TaxLotHoldingPeriod,
                   quantity: i128,
                   proceeds: i128,
                   basis: i128,
                   return_bps: i32| PortfolioClosedLot {
            lot_id: id.to_owned(),
            account_id: PortfolioAccountId::new("DEMO ACCOUNT"),
            instrument_id: InstrumentId::new(format!(
                "demo:instrument:{}",
                symbol.to_ascii_lowercase()
            )),
            symbol: symbol.to_owned(),
            acquired_date: acquired.to_owned(),
            disposed_date: disposed.to_owned(),
            holding_period: term,
            currency: usd,
            quantity: PositionQuantity::from_scaled_units(quantity),
            proceeds: Money::from_minor_units(proceeds, usd),
            cost_basis: Money::from_minor_units(basis, usd),
            realized_gain: Money::from_minor_units(proceeds - basis, usd),
            realized_return_bps: Some(return_bps),
        };
        PortfolioRealizedGainSnapshot {
            lots: vec![
                lot(
                    "DEMO-CLOSED-1",
                    "NVDA",
                    "2024-02-12",
                    "2026-03-06",
                    TaxLotHoldingPeriod::LongTerm,
                    40_000_000,
                    1_800_000,
                    900_000,
                    10_000,
                ),
                lot(
                    "DEMO-CLOSED-2",
                    "MSFT",
                    "2026-01-09",
                    "2026-07-17",
                    TaxLotHoldingPeriod::ShortTerm,
                    25_000_000,
                    1_050_000,
                    1_200_000,
                    -1_250,
                ),
            ],
            currency_totals: vec![PortfolioRealizedGainCurrencyTotal {
                currency: usd,
                lots: 2,
                proceeds: Money::from_minor_units(2_850_000, usd),
                cost_basis: Money::from_minor_units(2_100_000, usd),
                realized_gain: Money::from_minor_units(750_000, usd),
                short_term_gain: Money::from_minor_units(-150_000, usd),
                long_term_gain: Money::from_minor_units(900_000, usd),
                unknown_term_gain: Money::from_minor_units(0, usd),
            }],
            source: "DETERMINISTIC DEMO".to_owned(),
            period: "2026-03-06 — 2026-07-17".to_owned(),
            input_version: "DEMO-REALIZED-GAINS-V1".to_owned(),
            methodology: "DETERMINISTIC CLOSED-LOT FIXTURE · PROCEEDS − BASIS · USD".to_owned(),
            disclosures: vec![
                "DETERMINISTIC GALLERY CLOSED LOTS · NOT INTERACTIVE USER DATA".to_owned(),
                "BROKER-PROVIDED CLOSED LOTS · NOT TAX ADVICE".to_owned(),
            ],
        }
    }
}

fn demo_decimal(value: &str, decimals: u32) -> i128 {
    let negative = value.trim().starts_with('-');
    let cleaned = value
        .chars()
        .filter(|character| character.is_ascii_digit() || *character == '.')
        .collect::<String>();
    let (whole, fraction) = cleaned.split_once('.').unwrap_or((&cleaned, ""));
    let scale = 10_i128.pow(decimals);
    let mut result = whole.parse::<i128>().unwrap_or_default() * scale;
    let retained = fraction.chars().take(decimals as usize).collect::<String>();
    if !retained.is_empty() {
        result += retained.parse::<i128>().unwrap_or_default()
            * 10_i128.pow(decimals - retained.len() as u32);
    }
    if negative {
        -result
    } else {
        result
    }
}

impl NewsFeed for DemoData {
    fn load_news(&self) -> NewsSnapshot {
        NewsSnapshot {
            headlines: HEADLINES
                .iter()
                .map(|headline| Headline {
                    time: headline.0.to_owned(),
                    topic: headline.1.to_owned(),
                    title: headline.2.to_owned(),
                    region: headline.3.to_owned(),
                })
                .collect(),
        }
    }
}
