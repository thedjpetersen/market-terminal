use ratatui::layout::{Constraint, Layout, Rect};

#[derive(Debug, Clone, Copy)]
pub(super) struct AlertAreas {
    pub header: Rect,
    pub rules: Rect,
    pub audit: Rect,
    pub controls: Rect,
    pub disclosure: Rect,
}

pub(super) fn alert_areas(area: Rect) -> AlertAreas {
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(7),
        Constraint::Length(2),
    ])
    .split(area);
    AlertAreas {
        header: rows[0],
        rules: rows[1],
        audit: rows[2],
        controls: Rect::new(rows[3].x, rows[3].y, rows[3].width, rows[3].height.min(1)),
        disclosure: Rect::new(
            rows[3].x,
            rows[3].y.saturating_add(1),
            rows[3].width,
            rows[3].height.saturating_sub(1).min(1),
        ),
    }
}

pub(super) fn panel_header_area(area: Rect) -> Rect {
    Rect::new(area.x, area.y, area.width, area.height.min(1))
}

pub(super) fn table_row_area(area: Rect, index: usize) -> Option<Rect> {
    let y = area.y.saturating_add(3).saturating_add(index as u16);
    (area.width > 2 && y < area.bottom())
        .then(|| Rect::new(area.x.saturating_add(1), y, area.width.saturating_sub(2), 1))
}

#[cfg(test)]
pub(super) fn visible_rule_rows(area: Rect, rule_count: usize) -> usize {
    usize::from(area.height.saturating_sub(4)).min(rule_count)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AlertControl {
    Toggle,
    Acknowledge,
    Security,
    Refresh,
}

impl AlertControl {
    pub const ALL: [Self; 4] = [
        Self::Toggle,
        Self::Acknowledge,
        Self::Security,
        Self::Refresh,
    ];

    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Acknowledge => "acknowledge",
            Self::Security => "security",
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

pub(super) fn pack_control_areas(
    area: Rect,
    controls: impl IntoIterator<Item = (AlertControl, u16)>,
) -> Vec<(AlertControl, Rect)> {
    if area.height == 0 {
        return Vec::new();
    }
    let mut x = area.x;
    let right = area.right();
    let mut packed = Vec::new();
    for (control, width) in controls {
        if width == 0 || x.saturating_add(width) > right {
            break;
        }
        packed.push((control, Rect::new(x, area.y, width, 1)));
        x = x.saturating_add(width);
    }
    packed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_regions_remain_inside_supported_viewports() {
        for area in [Rect::new(0, 0, 80, 24), Rect::new(3, 4, 120, 36)] {
            let areas = alert_areas(area);
            for region in [
                areas.header,
                areas.rules,
                areas.audit,
                areas.controls,
                areas.disclosure,
            ] {
                assert!(region.x >= area.x);
                assert!(region.y >= area.y);
                assert!(region.right() <= area.right());
                assert!(region.bottom() <= area.bottom());
            }
        }
    }

    #[test]
    fn table_and_control_geometry_never_emit_partial_targets() {
        let packed = pack_control_areas(
            Rect::new(2, 3, 16, 1),
            [
                (AlertControl::Toggle, 8),
                (AlertControl::Acknowledge, 7),
                (AlertControl::Refresh, 5),
            ],
        );
        assert_eq!(packed.len(), 2);
        assert_eq!(packed[1].1.right(), 17);
        assert_eq!(visible_rule_rows(Rect::new(0, 0, 30, 7), 10), 3);
        assert!(table_row_area(Rect::new(0, 0, 30, 4), 1).is_none());
    }
}
