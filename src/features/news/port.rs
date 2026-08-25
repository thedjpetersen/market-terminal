use super::{NewsSnapshot, NewsWorkbench};

pub trait NewsQuery: Send + Sync {
    fn load_news(&self) -> NewsSnapshot;

    /// Adapters may override this with full text, calendars, and entitlement-
    /// aware results. The default keeps offline and test adapters useful.
    fn load_workbench(&self) -> NewsWorkbench {
        NewsWorkbench::from_snapshot(self.load_news())
    }
}
