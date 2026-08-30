use ratatui::layout::{Constraint, Layout, Rect};

#[derive(Debug, Clone, Copy)]
pub(super) struct GalleryAreas {
    pub header: Rect,
    pub returns: Rect,
    pub risk: Rect,
    pub composition: Rect,
    pub news: Rect,
    pub footer: Rect,
}

pub(super) fn gallery_areas(area: Rect) -> GalleryAreas {
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(21),
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Min(9),
        Constraint::Length(1),
    ])
    .split(area);
    GalleryAreas {
        header: rows[0],
        returns: rows[1],
        risk: rows[2],
        composition: rows[3],
        news: rows[4],
        footer: rows[5],
    }
}

pub(super) fn period_areas(area: Rect, periods: &[&str]) -> Vec<(usize, Rect)> {
    if area.height == 0 {
        return Vec::new();
    }
    let mut x = area.x.saturating_add(1);
    let right = area.right();
    let mut result = Vec::with_capacity(periods.len());
    for (index, period) in periods.iter().enumerate() {
        let width = format!(" {} {period} ", index + 1).chars().count() as u16;
        if width == 0 || x.saturating_add(width) > right {
            break;
        }
        result.push((index, Rect::new(x, area.y, width, 1)));
        x = x.saturating_add(width);
    }
    result
}

pub(super) fn panel_header_area(area: Rect) -> Rect {
    Rect::new(area.x, area.y, area.width, area.height.min(1))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OverviewControl {
    Portfolio,
    Risk,
    News,
    Refresh,
}

impl OverviewControl {
    pub const ALL: [Self; 4] = [Self::Portfolio, Self::Risk, Self::News, Self::Refresh];

    pub(super) const fn text(self) -> &'static str {
        match self {
            Self::Portfolio => " PORTFOLIO ",
            Self::Risk => " RISK ",
            Self::News => " NEWS ",
            Self::Refresh => " R REFRESH ",
        }
    }

    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::Portfolio => "portfolio",
            Self::Risk => "risk",
            Self::News => "news",
            Self::Refresh => "refresh",
        }
    }

    pub(super) fn action_id(self) -> String {
        format!("control:{}", self.key())
    }

    pub(super) fn from_action_id(id: &str) -> Option<Self> {
        let key = id.strip_prefix("control:")?;
        Self::ALL.into_iter().find(|control| control.key() == key)
    }
}

pub(super) fn control_areas(area: Rect) -> Vec<(OverviewControl, Rect)> {
    if area.height == 0 {
        return Vec::new();
    }
    let mut x = area.x;
    let right = area.right();
    let mut result = Vec::with_capacity(OverviewControl::ALL.len());
    for control in OverviewControl::ALL {
        let width = control.text().chars().count() as u16;
        if width == 0 || x.saturating_add(width) > right {
            break;
        }
        result.push((control, Rect::new(x, area.y, width, 1)));
        x = x.saturating_add(width);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_layouts_keep_every_region_inside_the_viewport() {
        for area in [Rect::new(0, 0, 80, 24), Rect::new(2, 3, 120, 36)] {
            let gallery = gallery_areas(area);
            for region in [
                gallery.header,
                gallery.returns,
                gallery.risk,
                gallery.composition,
                gallery.news,
                gallery.footer,
            ] {
                assert!(region.x >= area.x);
                assert!(region.y >= area.y);
                assert!(region.right() <= area.right());
                assert!(region.bottom() <= area.bottom());
            }
        }
    }

    #[test]
    fn packing_emits_only_complete_periods_controls_and_rows() {
        let periods = period_areas(Rect::new(0, 0, 13, 1), &["1D", "1M", "6M"]);
        assert_eq!(periods.len(), 2);
        assert_eq!(periods[1].1.right(), 13);

        let controls = control_areas(Rect::new(0, 0, 18, 1));
        assert_eq!(controls.len(), 2);
        assert_eq!(controls[1].0, OverviewControl::Risk);
    }
}
