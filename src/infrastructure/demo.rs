use crate::features::{
    markets::{MarketIndex, MarketsQuery, MarketsSnapshot},
    news::{Headline, NewsQuery, NewsSnapshot},
    overview::{OverviewQuery, OverviewSnapshot},
    portfolio::{PortfolioQuery, PortfolioSnapshot, Position},
    security::{SecurityQuery, SecuritySnapshot},
};

#[derive(Debug, Default, Clone, Copy)]
pub struct DemoData;

const PERIODS: [&str; 8] = ["D", "M", "6M", "YTD", "1Y", "2Y", "5Y", "10Y"];
const RETURNS_A: [(f64, f64); 24] = [
    (0., 1.1), (1., 2.1), (5., 3.5), (10., 4.8), (12., 3.5), (18., 4.7),
    (25., 5.), (31., 3.3), (38., -0.1), (40., 1.), (45., 3.6), (47., 5.),
    (53., 7.7), (61., 9.), (65., 10.3), (70., 11.7), (73., 10.3), (79., 13.),
    (84., 14.4), (88., 11.7), (94., 14.4), (97., 15.8), (99., 14.5), (100., 17.1),
];
const RETURNS_B: [(f64, f64); 20] = [
    (0., 1.), (1., 2.2), (4., 3.5), (9., 2.1), (17., 3.5), (24., 2.2),
    (31., 2.1), (37., -1.7), (40., 0.9), (48., 5.), (50., 6.4), (62., 9.),
    (67., 10.4), (71., 9.), (75., 11.7), (82., 13.), (88., 10.3), (94., 13.),
    (97., 14.4), (100., 14.3),
];
const SECURITY_PRICE: [(f64, f64); 18] = [
    (0., 196.3), (5., 197.1), (10., 196.7), (16., 198.4), (21., 197.8),
    (27., 199.2), (33., 199.8), (39., 199.1), (45., 200.4), (52., 201.1),
    (58., 200.7), (64., 202.4), (70., 203.2), (76., 202.7), (82., 203.8),
    (88., 204.6), (94., 204.1), (100., 205.3),
];
const TREASURY_CURVE: [(f64, f64); 8] = [
    (0., 5.38), (14., 4.91), (28., 4.78), (43., 4.47), (57., 4.40),
    (72., 4.31), (86., 4.38), (100., 4.47),
];

const MARKETS: [MarketIndex; 7] = [
    MarketIndex { name: "S&P 500", symbol: "SPX", last: "5,304.72", net_change: "+45.18", percent_change: "+0.86%" },
    MarketIndex { name: "NASDAQ 100", symbol: "NDX", last: "18,658.32", net_change: "+183.72", percent_change: "+1.00%" },
    MarketIndex { name: "DOW JONES", symbol: "INDU", last: "39,512.88", net_change: "+305.18", percent_change: "+0.78%" },
    MarketIndex { name: "RUSSELL 2000", symbol: "RTY", last: "2,075.34", net_change: "+20.11", percent_change: "+0.98%" },
    MarketIndex { name: "STOXX 600", symbol: "SXXP", last: "514.22", net_change: "+2.04", percent_change: "+0.40%" },
    MarketIndex { name: "NIKKEI 225", symbol: "NKY", last: "38,487.90", net_change: "+1,724.25", percent_change: "+4.69%" },
    MarketIndex { name: "HANG SENG", symbol: "HSI", last: "18,028.57", net_change: "−150.58", percent_change: "−0.83%" },
];

const POSITIONS: [Position; 7] = [
    Position { symbol: "NVDA", quantity: "1,240", average_cost: "155.04", market_value: "192,250", pnl: "+42.18%", weight: "18.4%" },
    Position { symbol: "AAPL", quantity: "820", average_cost: "164.21", market_value: "168,346", pnl: "+25.02%", weight: "16.1%" },
    Position { symbol: "MSFT", quantity: "315", average_cost: "382.80", market_value: "141,422", pnl: "+17.30%", weight: "13.5%" },
    Position { symbol: "META", quantity: "228", average_cost: "421.14", market_value: "116,967", pnl: "+21.82%", weight: "11.2%" },
    Position { symbol: "AMZN", quantity: "540", average_cost: "156.08", market_value: "100,391", pnl: "+19.12%", weight: "9.6%" },
    Position { symbol: "VTI", quantity: "412", average_cost: "236.44", market_value: "111,231", pnl: "+14.18%", weight: "10.6%" },
    Position { symbol: "SGOV", quantity: "650", average_cost: "100.31", market_value: "65,208", pnl: "+1.03%", weight: "6.2%" },
];

const HEADLINES: [Headline; 10] = [
    Headline { time: "16:00", topic: "TOP", title: "Wall Street closes higher as chipmakers lead broad rally", region: "US" },
    Headline { time: "15:45", topic: "FED", title: "Fed's Barkin: inflation progress has resumed", region: "US" },
    Headline { time: "15:30", topic: "ECO", title: "Treasury yields fall after softer housing data", region: "US" },
    Headline { time: "15:16", topic: "CMD", title: "Oil prices slide on weaker demand outlook", region: "GL" },
    Headline { time: "14:58", topic: "EQU", title: "European markets mixed ahead of U.S. close", region: "EU" },
    Headline { time: "14:41", topic: "TEC", title: "Micron outlook revives AI trade; suppliers surge", region: "AS" },
    Headline { time: "14:22", topic: "FX", title: "Dollar slips as traders bring forward first rate cut", region: "GL" },
    Headline { time: "13:55", topic: "POL", title: "EU agrees strategic investment framework", region: "EU" },
    Headline { time: "13:31", topic: "EQU", title: "Small caps outperform for third session", region: "US" },
    Headline { time: "12:48", topic: "CMD", title: "Copper advances on China stimulus expectations", region: "AS" },
];

impl OverviewQuery for DemoData {
    fn load_overview(&self) -> OverviewSnapshot {
        OverviewSnapshot { periods: &PERIODS, primary_returns: &RETURNS_A, comparison_returns: &RETURNS_B }
    }
}

impl MarketsQuery for DemoData {
    fn load_markets(&self) -> MarketsSnapshot {
        MarketsSnapshot { indices: &MARKETS, treasury_curve: &TREASURY_CURVE }
    }
}

impl SecurityQuery for DemoData {
    fn load_security(&self, _symbol: &str) -> SecuritySnapshot {
        SecuritySnapshot {
            symbol: "AAPL US EQUITY",
            name: "APPLE INC",
            last: "205.30",
            absolute_change: "+1.72",
            percent_change: "+0.84%",
            session_summary: "OPEN 203.41  HIGH 205.64  LOW 202.72  VOLUME 41.82M",
            price_series: &SECURITY_PRICE,
        }
    }
}

impl PortfolioQuery for DemoData {
    fn load_portfolio(&self) -> PortfolioSnapshot {
        PortfolioSnapshot {
            positions: &POSITIONS,
            net_asset_value: "$1,045,228",
            ytd_return: "+17.02%",
            available_cash: "$127,834",
            sharpe: "2.79",
        }
    }
}

impl NewsQuery for DemoData {
    fn load_news(&self) -> NewsSnapshot { NewsSnapshot { headlines: &HEADLINES } }
}
