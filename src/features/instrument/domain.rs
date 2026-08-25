pub use crate::foundation::InstrumentId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentKind {
    Equity,
    Etf,
    Index,
    Currency,
    Commodity,
}

impl InstrumentKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Equity => "EQUITY",
            Self::Etf => "ETF",
            Self::Index => "INDEX",
            Self::Currency => "FX",
            Self::Commodity => "COMMODITY",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrument {
    pub id: InstrumentId,
    pub symbol: String,
    pub name: String,
    pub venue: String,
    pub currency: String,
    pub kind: InstrumentKind,
}

impl Instrument {
    pub fn terminal_subject(&self) -> String {
        match self.kind {
            InstrumentKind::Equity | InstrumentKind::Etf => {
                format!("{} {}", self.symbol, self.venue)
            }
            _ => self.symbol.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_instruments_include_venue_in_terminal_subject() {
        let instrument = Instrument {
            id: InstrumentId::new("us:xnas:aapl"),
            symbol: "AAPL".to_owned(),
            name: "Apple Inc.".to_owned(),
            venue: "US".to_owned(),
            currency: "USD".to_owned(),
            kind: InstrumentKind::Equity,
        };
        assert_eq!(instrument.terminal_subject(), "AAPL US");
    }
}
