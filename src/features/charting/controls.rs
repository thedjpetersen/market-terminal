use ratatui::layout::{Constraint, Layout, Rect};

use super::ChartPeriod;

#[derive(Debug, Clone, Copy)]
pub(super) struct ChartAreas {
    pub header: Rect,
    pub plot: Rect,
    pub footer: Rect,
}

pub(super) fn chart_areas(area: Rect) -> ChartAreas {
    let sections = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(3),
    ])
    .split(area);
    ChartAreas {
        header: sections[0],
        plot: sections[1],
        footer: sections[2],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChartControl {
    Period(ChartPeriod),
    Normalization,
    MovingAverages,
    AverageKind,
    Rsi,
    Volume,
    Comparison,
    ClearComparisons,
    InspectBack,
    InspectForward,
    Latest,
    DisplayMode,
    LineMode,
    InsertSheet,
    Refresh,
}

impl ChartControl {
    pub(super) fn action_id(self) -> String {
        match self {
            Self::Period(period) => format!("period:{}", period.label()),
            Self::Normalization => "control:normalization".to_owned(),
            Self::MovingAverages => "control:moving-averages".to_owned(),
            Self::AverageKind => "control:average-kind".to_owned(),
            Self::Rsi => "control:rsi".to_owned(),
            Self::Volume => "control:volume".to_owned(),
            Self::Comparison => "control:comparison".to_owned(),
            Self::ClearComparisons => "control:clear-comparisons".to_owned(),
            Self::InspectBack => "control:inspect-back".to_owned(),
            Self::InspectForward => "control:inspect-forward".to_owned(),
            Self::Latest => "control:latest".to_owned(),
            Self::DisplayMode => "control:display-mode".to_owned(),
            Self::LineMode => "control:line-mode".to_owned(),
            Self::InsertSheet => "control:insert-sheet".to_owned(),
            Self::Refresh => "control:refresh".to_owned(),
        }
    }

    pub(super) fn from_action_id(id: &str) -> Option<Self> {
        if let Some(period) = id.strip_prefix("period:").and_then(ChartPeriod::parse) {
            return Some(Self::Period(period));
        }
        match id {
            "control:normalization" => Some(Self::Normalization),
            "control:moving-averages" => Some(Self::MovingAverages),
            "control:average-kind" => Some(Self::AverageKind),
            "control:rsi" => Some(Self::Rsi),
            "control:volume" => Some(Self::Volume),
            "control:comparison" => Some(Self::Comparison),
            "control:clear-comparisons" => Some(Self::ClearComparisons),
            "control:inspect-back" => Some(Self::InspectBack),
            "control:inspect-forward" => Some(Self::InspectForward),
            "control:latest" => Some(Self::Latest),
            "control:display-mode" => Some(Self::DisplayMode),
            "control:line-mode" => Some(Self::LineMode),
            "control:insert-sheet" => Some(Self::InsertSheet),
            "control:refresh" => Some(Self::Refresh),
            _ => None,
        }
    }
}

/// Packs complete controls from left to right, then top to bottom.
///
/// Controls that cannot fit in one row are omitted. Once the footer is full,
/// remaining controls are omitted as a unit so rendering and hit testing never
/// disagree over a partially visible destination.
pub(super) fn pack_control_areas(
    area: Rect,
    controls: impl IntoIterator<Item = (ChartControl, u16)>,
) -> Vec<(ChartControl, Rect)> {
    let right = area.x.saturating_add(area.width);
    let bottom = area.y.saturating_add(area.height);
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
    fn packing_wraps_whole_controls_and_stops_at_the_viewport() {
        let area = Rect::new(4, 7, 8, 2);
        let controls = [
            (ChartControl::Period(ChartPeriod::OneDay), 5),
            (ChartControl::Normalization, 5),
            (ChartControl::Refresh, 9),
            (ChartControl::Volume, 3),
        ];

        assert_eq!(
            pack_control_areas(area, controls),
            vec![
                (
                    ChartControl::Period(ChartPeriod::OneDay),
                    Rect::new(4, 7, 5, 1),
                ),
                (ChartControl::Normalization, Rect::new(4, 8, 5, 1)),
                (ChartControl::Volume, Rect::new(9, 8, 3, 1)),
            ]
        );
    }
}
