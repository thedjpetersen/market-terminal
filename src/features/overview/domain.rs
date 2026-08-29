#[derive(Debug, Clone)]
pub enum OverviewSnapshot {
    Gallery {
        periods: &'static [&'static str],
        primary_returns: &'static [(f64, f64)],
        comparison_returns: &'static [(f64, f64)],
    },
    Live(Box<LiveOverviewSnapshot>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveOverviewSnapshot {
    pub net_asset_value: String,
    pub ytd_return: String,
    pub available_cash: String,
    pub sharpe: String,
    pub portfolio_source: String,
    pub portfolio_as_of: String,
    pub holdings: Vec<OverviewHolding>,
    pub headlines: Vec<OverviewHeadline>,
    pub news_status: String,
    pub market_pulse: Vec<OverviewMarketPulse>,
    pub events: Vec<OverviewEvent>,
    pub source_health: Vec<OverviewSourceHealth>,
    pub saved_work: Vec<OverviewSavedWork>,
    pub priorities: Vec<OverviewPriority>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewHolding {
    pub symbol: String,
    pub quantity: String,
    pub market_value: String,
    pub pnl: String,
    pub weight: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewHeadline {
    pub time: String,
    pub topic: String,
    pub title: String,
    pub region: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewMarketPulse {
    pub symbol: String,
    pub last: String,
    pub percent_change: String,
    pub quality: String,
    pub as_of: String,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewEvent {
    pub time: String,
    pub region: String,
    pub importance: String,
    pub title: String,
    pub period: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewHealthState {
    Ready,
    Partial,
    Loading,
    Unavailable,
    NotConfigured,
}

impl OverviewHealthState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "READY",
            Self::Partial => "PARTIAL",
            Self::Loading => "LOADING",
            Self::Unavailable => "UNAVAILABLE",
            Self::NotConfigured => "NOT CONFIGURED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewSourceHealth {
    pub source: String,
    pub state: OverviewHealthState,
    pub detail: String,
    pub as_of: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewSavedWork {
    pub id: u64,
    pub label: String,
    pub command: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewPriority {
    pub id: String,
    pub score: u16,
    pub title: String,
    pub reason: String,
    pub source: String,
    pub as_of: String,
    pub command: String,
}
