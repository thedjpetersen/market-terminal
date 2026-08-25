#[derive(Debug, Clone, Copy)]
pub struct SecuritySnapshot {
    pub symbol: &'static str,
    pub name: &'static str,
    pub last: &'static str,
    pub absolute_change: &'static str,
    pub percent_change: &'static str,
    pub session_summary: &'static str,
    pub price_series: &'static [(f64, f64)],
}
