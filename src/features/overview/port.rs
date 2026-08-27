use super::OverviewSnapshot;

pub trait OverviewQuery: Send + Sync {
    fn load_overview(&self) -> OverviewSnapshot;

    fn request_refresh(&self) {}
}
