use crate::foundation::InstrumentId;

#[derive(Debug, Clone, PartialEq)]
pub struct SecuritySnapshot {
    pub symbol: String,
    pub name: String,
    pub last: String,
    pub absolute_change: String,
    pub percent_change: String,
    pub session_summary: String,
    pub price_series: Vec<(f64, f64)>,
    pub statistics: Vec<(String, String)>,
    pub source: String,
}

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
        let instrument_id =
            InstrumentId::new(format!("{venue}:listed:{}", ticker.to_ascii_lowercase()));
        Self {
            instrument_id,
            terminal_symbol: format!("{ticker} {}", venue.to_ascii_uppercase()),
        }
    }

    pub fn from_sec_cik(cik: u64, ticker: &str) -> Self {
        Self {
            instrument_id: InstrumentId::new(format!("sec:cik:{cik:010}")),
            terminal_symbol: format!("{} US", ticker.to_ascii_uppercase()),
        }
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
            Self::Ownership => "OWN FORM 4",
            Self::Filings => "FIL FILINGS",
            Self::Peers => "RV PEERS",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FinancialPeriod {
    pub period: String,
    pub revenue_billions: String,
    pub operating_income_billions: String,
    pub net_income_billions: String,
    pub diluted_eps: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    pub period: String,
    pub revenue: String,
    pub eps: String,
    pub eps_high: String,
    pub eps_low: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerPosition {
    pub manager: String,
    pub shares: String,
    pub value: String,
    pub quarterly_change: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InsiderTransaction {
    pub filed: String,
    pub transaction_date: String,
    pub owner: String,
    pub role: String,
    pub transaction_code: String,
    pub acquisition_disposition: String,
    pub shares: f64,
    pub price_per_share: Option<f64>,
    pub value_usd: Option<f64>,
    pub shares_after: Option<f64>,
    pub ownership_nature: String,
    pub plan_10b5_1: bool,
    pub accession: String,
    pub document_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filing {
    pub filed: String,
    pub form: String,
    pub period: String,
    pub description: String,
    pub accession: String,
    pub document_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PeerComparison {
    pub symbol: String,
    pub name: String,
    pub price_to_earnings: String,
    pub ev_to_ebitda: String,
    pub revenue_growth: String,
    pub gross_margin: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SecurityResearch {
    pub identity: SecurityIdentity,
    pub financials: Vec<FinancialPeriod>,
    pub estimates: Vec<Estimate>,
    pub owners: Vec<OwnerPosition>,
    pub insider_transactions: Vec<InsiderTransaction>,
    pub insider_status: String,
    pub filings: Vec<Filing>,
    pub peers: Vec<PeerComparison>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SecurityPage {
    pub snapshot: SecuritySnapshot,
    pub research: SecurityResearch,
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
    fn sec_cik_is_a_stable_canonical_identity() {
        let identity = SecurityIdentity::from_sec_cik(320_193, "aapl");
        assert_eq!(identity.instrument_id.as_str(), "sec:cik:0000320193");
        assert_eq!(identity.terminal_symbol, "AAPL US");
    }

    #[test]
    fn research_views_cycle_without_leaking_ui_state_into_domain_data() {
        assert_eq!(ResearchView::Peers.next(), ResearchView::Financials);
        assert_eq!(ResearchView::Financials.next(), ResearchView::Estimates);
    }
}
