use super::MarketsSnapshot;

pub trait MarketsQuery: Send + Sync {
    fn load_markets(&self) -> MarketsSnapshot;

    fn request_refresh(&self) {}
}
