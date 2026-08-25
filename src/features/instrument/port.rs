use super::domain::Instrument;

pub trait InstrumentSearch: Send + Sync {
    fn search(&self, query: &str, limit: usize) -> Vec<Instrument>;
}
