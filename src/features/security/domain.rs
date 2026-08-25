use crate::foundation::InstrumentId;

#[derive(Debug, Clone, Copy)]
pub struct SecuritySnapshot {
    pub symbol: &'static str,
    pub name: &'static str,
    pub last: &'static str,
    pub absolute_change: &'static str,
    pub percent_change: &'static str,
    pub session_summary: &'static str,
    pub price_series: &'static [(f64, f64)],
}

/// Stable research-page identity. Presentation symbols may change without
/// invalidating saved links to filings, estimates, or news.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityIdentity {
    pub instrument_id: InstrumentId,
    pub terminal_symbol: String,
}

impl SecurityIdentity {
    pub fn from_terminal_symbol(symbol: &str) -> Self {
        let mut tokens = symbol.split_whitespace();
        let ticker = tokens.next().unwrap_or("AAPL").to_ascii_uppercase();
        let venue = tokens.next().unwrap_or("US").to_ascii_lowercase();
        let instrument_id = InstrumentId::new(format!("{venue}:listed:{}", ticker.to_ascii_lowercase()));
        Self { instrument_id, terminal_symbol: format!("{ticker} {}", venue.to_ascii_uppercase()) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResearchView {
    Financials,
    Estimates,
    Ownership,
    Filings,
    Peers,
}

impl ResearchView {
    pub const ALL: [Self; 5] = [
        Self::Financials,
        Self::Estimates,
        Self::Ownership,
        Self::Filings,
        Self::Peers,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Financials => "FA FINANCIALS",
            Self::Estimates => "EE ESTIMATES",
            Self::Ownership => "OWN OWNERSHIP",
            Self::Filings => "FIL FILINGS",
            Self::Peers => "RV PEERS",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|candidate| *candidate == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub period: &'static str,
    pub revenue: &'static str,
    pub eps: &'static str,
    pub eps_high: &'static str,
    pub eps_low: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerPosition {
    pub manager: &'static str,
    pub shares: &'static str,
    pub value: &'static str,
    pub quarterly_change: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filing {
    pub filed: &'static str,
    pub form: &'static str,
    pub period: &'static str,
    pub description: &'static str,
    pub accession: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeerComparison {
    pub symbol: &'static str,
    pub name: &'static str,
    pub price_to_earnings: &'static str,
    pub ev_to_ebitda: &'static str,
    pub revenue_growth: &'static str,
    pub gross_margin: &'static str,
}

#[derive(Debug, Clone)]
pub struct SecurityResearch {
    pub identity: SecurityIdentity,
    pub estimates: &'static [Estimate],
    pub owners: &'static [OwnerPosition],
    pub filings: &'static [Filing],
    pub peers: &'static [PeerComparison],
}

const ESTIMATES: [Estimate; 4] = [
    Estimate { period: "FY24A", revenue: "391.0B", eps: "6.57", eps_high: "—", eps_low: "—" },
    Estimate { period: "FY25E", revenue: "414.8B", eps: "7.24", eps_high: "7.48", eps_low: "6.98" },
    Estimate { period: "FY26E", revenue: "438.1B", eps: "7.93", eps_high: "8.34", eps_low: "7.51" },
    Estimate { period: "FY27E", revenue: "464.5B", eps: "8.71", eps_high: "9.20", eps_low: "8.09" },
];

const OWNERS: [OwnerPosition; 5] = [
    OwnerPosition { manager: "VANGUARD GROUP", shares: "1.32B", value: "$270.8B", quarterly_change: "+0.8%" },
    OwnerPosition { manager: "BLACKROCK", shares: "1.04B", value: "$213.5B", quarterly_change: "+0.3%" },
    OwnerPosition { manager: "BERKSHIRE HATHAWAY", shares: "300.0M", value: "$61.6B", quarterly_change: "−6.2%" },
    OwnerPosition { manager: "STATE STREET", shares: "585.1M", value: "$120.1B", quarterly_change: "+1.1%" },
    OwnerPosition { manager: "GEODE CAPITAL", shares: "325.9M", value: "$66.9B", quarterly_change: "+2.4%" },
];

const FILINGS: [Filing; 5] = [
    Filing { filed: "2026-08-01", form: "10-Q", period: "2026-06-27", description: "QUARTERLY REPORT", accession: "0000320193-26-000081" },
    Filing { filed: "2026-05-02", form: "10-Q", period: "2026-03-28", description: "QUARTERLY REPORT", accession: "0000320193-26-000052" },
    Filing { filed: "2026-03-14", form: "8-K", period: "2026-03-14", description: "MATERIAL EVENT", accession: "0000320193-26-000038" },
    Filing { filed: "2026-01-30", form: "8-K", period: "2026-01-30", description: "RESULTS / GUIDANCE", accession: "0000320193-26-000021" },
    Filing { filed: "2025-11-01", form: "10-K", period: "2025-09-27", description: "ANNUAL REPORT", accession: "0000320193-25-000119" },
];

const PEERS: [PeerComparison; 5] = [
    PeerComparison { symbol: "AAPL", name: "APPLE", price_to_earnings: "29.4x", ev_to_ebitda: "22.8x", revenue_growth: "+6.1%", gross_margin: "46.3%" },
    PeerComparison { symbol: "MSFT", name: "MICROSOFT", price_to_earnings: "31.8x", ev_to_ebitda: "23.6x", revenue_growth: "+14.2%", gross_margin: "69.8%" },
    PeerComparison { symbol: "GOOGL", name: "ALPHABET", price_to_earnings: "23.1x", ev_to_ebitda: "16.2x", revenue_growth: "+12.7%", gross_margin: "58.4%" },
    PeerComparison { symbol: "META", name: "META", price_to_earnings: "24.8x", ev_to_ebitda: "15.9x", revenue_growth: "+15.1%", gross_margin: "81.2%" },
    PeerComparison { symbol: "AMZN", name: "AMAZON", price_to_earnings: "28.7x", ev_to_ebitda: "17.5x", revenue_growth: "+10.8%", gross_margin: "49.6%" },
];

impl SecurityResearch {
    pub fn deterministic(symbol: &str) -> Self {
        Self {
            identity: SecurityIdentity::from_terminal_symbol(symbol),
            estimates: &ESTIMATES,
            owners: &OWNERS,
            filings: &FILINGS,
            peers: &PEERS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_symbol_maps_to_canonical_identity() {
        let identity = SecurityIdentity::from_terminal_symbol("aapl US EQUITY");
        assert_eq!(identity.instrument_id.as_str(), "us:listed:aapl");
        assert_eq!(identity.terminal_symbol, "AAPL US");
    }

    #[test]
    fn research_views_cycle_without_leaking_ui_state_into_domain_data() {
        assert_eq!(ResearchView::Peers.next(), ResearchView::Financials);
        assert_eq!(ResearchView::Financials.next(), ResearchView::Estimates);
    }
}
