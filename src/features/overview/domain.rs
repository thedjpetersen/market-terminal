#[derive(Debug, Clone, Copy)]
pub struct OverviewSnapshot {
    pub periods: &'static [&'static str],
    pub primary_returns: &'static [(f64, f64)],
    pub comparison_returns: &'static [(f64, f64)],
}
