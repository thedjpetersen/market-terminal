use super::domain::Instrument;

pub trait InstrumentSearch: Send + Sync {
    fn search(&self, query: &str, limit: usize) -> Vec<Instrument>;

    fn revision(&self) -> u64 {
        0
    }

    fn status(&self) -> String {
        "INSTRUMENT CATALOG READY".to_owned()
    }

    fn request_refresh(&self) {}
}
