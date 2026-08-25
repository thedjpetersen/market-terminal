#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Headline {
    pub time: &'static str,
    pub topic: &'static str,
    pub title: &'static str,
    pub region: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct NewsSnapshot {
    pub headlines: &'static [Headline],
}
