use super::NewsSnapshot;

pub trait NewsQuery: Send + Sync {
    fn load_news(&self) -> NewsSnapshot;
}
