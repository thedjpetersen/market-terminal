use ratatui::layout::{Constraint, Layout, Rect};

pub(super) const WIDE_NEWS_MIN_COLUMNS: u16 = 90;

#[derive(Debug, Clone, Copy)]
pub(super) struct NewsAreas {
    pub header: Rect,
    pub summary: Rect,
    pub controls: Rect,
    pub body: Rect,
    pub stories: Rect,
    pub detail: Option<Rect>,
    pub events: Option<Rect>,
}

pub(super) fn news_areas(area: Rect) -> NewsAreas {
    let rows = Layout::vertical([Constraint::Length(5), Constraint::Min(10)]).split(area);
    let summary = Rect::new(
        rows[0].x.saturating_add(1),
        rows[0].y.saturating_add(1),
        rows[0].width.saturating_sub(2),
        1.min(rows[0].height.saturating_sub(2)),
    );
    let controls = Rect::new(
        summary.x,
        summary.y.saturating_add(1),
        summary.width,
        rows[0].height.saturating_sub(3),
    );
    if rows[1].width < WIDE_NEWS_MIN_COLUMNS {
        return NewsAreas {
            header: rows[0],
            summary,
            controls,
            body: rows[1],
            stories: rows[1],
            detail: None,
            events: None,
        };
    }
    let columns = Layout::horizontal([
        Constraint::Percentage(39),
        Constraint::Percentage(43),
        Constraint::Percentage(18),
    ])
    .split(rows[1]);
    NewsAreas {
        header: rows[0],
        summary,
        controls,
        body: rows[1],
        stories: columns[0],
        detail: Some(columns[1]),
        events: Some(columns[2]),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NewsControl {
    Reset,
    RegionUs,
    RegionEu,
    RegionAsia,
    Unread,
    Saved,
    Calendar,
    ReadState,
    Bookmark,
    Security,
    InsertSheet,
    Refresh,
}

impl NewsControl {
    pub const ALL: [Self; 12] = [
        Self::Reset,
        Self::RegionUs,
        Self::RegionEu,
        Self::RegionAsia,
        Self::Unread,
        Self::Saved,
        Self::Calendar,
        Self::ReadState,
        Self::Bookmark,
        Self::Security,
        Self::InsertSheet,
        Self::Refresh,
    ];

    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::Reset => "reset",
            Self::RegionUs => "region-us",
            Self::RegionEu => "region-eu",
            Self::RegionAsia => "region-asia",
            Self::Unread => "unread",
            Self::Saved => "saved",
            Self::Calendar => "calendar",
            Self::ReadState => "read-state",
            Self::Bookmark => "bookmark",
            Self::Security => "security",
            Self::InsertSheet => "insert-sheet",
            Self::Refresh => "refresh",
        }
    }

    pub(super) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|control| control.key() == key)
    }

    pub(super) const fn is_story_specific(self) -> bool {
        matches!(
            self,
            Self::ReadState | Self::Bookmark | Self::Security | Self::InsertSheet
        )
    }
}

pub(super) fn pack_control_areas(
    area: Rect,
    controls: impl IntoIterator<Item = (NewsControl, u16)>,
) -> Vec<(NewsControl, Rect)> {
    let right = area.right();
    let bottom = area.bottom();
    let mut x = area.x;
    let mut y = area.y;
    let mut areas = Vec::new();
    for (control, width) in controls {
        if width == 0 || width > area.width {
            continue;
        }
        if x.saturating_add(width) > right {
            x = area.x;
            y = y.saturating_add(1);
        }
        if y >= bottom {
            break;
        }
        areas.push((control, Rect::new(x, y, width, 1)));
        x = x.saturating_add(width);
    }
    areas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_keep_controls_and_body_regions_inside_each_viewport() {
        let narrow = news_areas(Rect::new(3, 4, 80, 24));
        assert_eq!(narrow.header.height, 5);
        assert_eq!(narrow.stories, narrow.body);
        assert!(narrow.detail.is_none());
        assert!(narrow.events.is_none());

        let wide = news_areas(Rect::new(3, 4, 120, 36));
        assert!(wide.detail.is_some());
        assert!(wide.events.is_some());
        assert_eq!(wide.body.width, 120);
    }

    #[test]
    fn control_packing_never_emits_partial_destinations() {
        let area = Rect::new(2, 3, 10, 2);
        let controls = [
            (NewsControl::Reset, 6),
            (NewsControl::RegionUs, 6),
            (NewsControl::Refresh, 11),
            (NewsControl::Unread, 4),
        ];
        assert_eq!(
            pack_control_areas(area, controls),
            vec![
                (NewsControl::Reset, Rect::new(2, 3, 6, 1)),
                (NewsControl::RegionUs, Rect::new(2, 4, 6, 1)),
                (NewsControl::Unread, Rect::new(8, 4, 4, 1)),
            ]
        );
    }
}
