use super::SecuritySnapshot;

pub trait SecurityQuery: Send + Sync {
    fn load_security(&self, symbol: &str) -> SecuritySnapshot;
}
