use ratatui::layout::{Constraint, Layout, Rect};

use crate::features::spreadsheet::domain::{MAX_COLUMNS, MAX_ROWS};

pub(super) const CELL_WIDTH: u16 = 12;
pub(super) const ROW_HEADER_WIDTH: u16 = 5;

#[derive(Debug, Clone, Copy)]
pub(super) struct SpreadsheetAreas {
    pub formula: Rect,
    pub grid: Rect,
    pub tabs: Rect,
    pub controls: Rect,
    pub status: Rect,
}

pub(super) fn spreadsheet_areas(area: Rect) -> SpreadsheetAreas {
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .split(area);
    SpreadsheetAreas {
        formula: rows[0],
        grid: rows[1],
        tabs: rows[2],
        controls: rows[3],
        status: rows[4],
    }
}

pub(super) fn formula_action_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2).min(1),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GridGeometry {
    pub area: Rect,
    pub first_column: u8,
    pub first_row: u16,
    pub columns: u8,
    pub rows: u16,
}

impl GridGeometry {
    pub(super) fn new(area: Rect, first_column: u8, first_row: u16) -> Self {
        let available_width = area.width.saturating_sub(ROW_HEADER_WIDTH + 3);
        let columns = (available_width / (CELL_WIDTH + 1))
            .max(1)
            .min(u16::from(MAX_COLUMNS - first_column + 1)) as u8;
        let rows = area
            .height
            .saturating_sub(3)
            .max(1)
            .min(MAX_ROWS - first_row + 1);
        Self {
            area,
            first_column,
            first_row,
            columns,
            rows,
        }
    }

    pub(super) fn contains(self, column: u8, row: u16) -> bool {
        column >= self.first_column
            && column < self.first_column.saturating_add(self.columns)
            && row >= self.first_row
            && row < self.first_row.saturating_add(self.rows)
    }

    pub(super) fn cell_area(self, column: u8, row: u16) -> Option<Rect> {
        if !self.contains(column, row) {
            return None;
        }
        let column_offset = u16::from(column - self.first_column);
        let row_offset = row - self.first_row;
        Some(Rect::new(
            self.area
                .x
                .saturating_add(ROW_HEADER_WIDTH + 2)
                .saturating_add(column_offset.saturating_mul(CELL_WIDTH + 1)),
            self.area.y.saturating_add(2).saturating_add(row_offset),
            CELL_WIDTH,
            1,
        ))
    }

    pub(super) fn row_header_area(self, row: u16) -> Option<Rect> {
        if row < self.first_row || row >= self.first_row.saturating_add(self.rows) {
            return None;
        }
        Some(Rect::new(
            self.area.x.saturating_add(1),
            self.area
                .y
                .saturating_add(2)
                .saturating_add(row - self.first_row),
            ROW_HEADER_WIDTH,
            1,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SpreadsheetControl {
    Edit,
    Clear,
    Copy,
    Paste,
    FillDown,
    FillRight,
    Undo,
    Redo,
    Security,
    Chart,
    News,
    Refresh,
}

impl SpreadsheetControl {
    pub const ALL: [Self; 12] = [
        Self::Edit,
        Self::Clear,
        Self::Copy,
        Self::Paste,
        Self::FillDown,
        Self::FillRight,
        Self::Undo,
        Self::Redo,
        Self::Security,
        Self::Chart,
        Self::News,
        Self::Refresh,
    ];

    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Clear => "clear",
            Self::Copy => "copy",
            Self::Paste => "paste",
            Self::FillDown => "fill-down",
            Self::FillRight => "fill-right",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Security => "security",
            Self::Chart => "chart",
            Self::News => "news",
            Self::Refresh => "refresh",
        }
    }

    pub(super) const fn text(self) -> &'static str {
        match self {
            Self::Edit => " ENTER EDIT ",
            Self::Clear => " DEL CLEAR ",
            Self::Copy => " Y COPY ",
            Self::Paste => " P PASTE ",
            Self::FillDown => " CTRL-D DOWN ",
            Self::FillRight => " CTRL-R RIGHT ",
            Self::Undo => " CTRL-Z UNDO ",
            Self::Redo => " CTRL-Y REDO ",
            Self::Security => " S SECURITY ",
            Self::Chart => " C CHART ",
            Self::News => " N NEWS ",
            Self::Refresh => " F9 REFRESH ",
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

pub(super) fn pack_control_areas(area: Rect) -> Vec<(SpreadsheetControl, Rect)> {
    pack_wrapped(
        area,
        SpreadsheetControl::ALL
            .into_iter()
            .map(|control| (control, control.text().chars().count() as u16)),
    )
}

pub(super) fn pack_tab_areas(
    area: Rect,
    widths: impl IntoIterator<Item = (usize, u16)>,
) -> Vec<(usize, Rect)> {
    pack_wrapped(area, widths)
}

fn pack_wrapped<T: Copy>(area: Rect, items: impl IntoIterator<Item = (T, u16)>) -> Vec<(T, Rect)> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let mut x = area.x;
    let mut y = area.y;
    let mut packed = Vec::new();
    for (item, width) in items {
        if width == 0 || width > area.width {
            continue;
        }
        if x.saturating_add(width) > area.right() {
            y = y.saturating_add(1);
            x = area.x;
        }
        if y >= area.bottom() {
            break;
        }
        packed.push((item, Rect::new(x, y, width, 1)));
        x = x.saturating_add(width);
    }
    packed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_layouts_keep_every_region_and_cell_inside_the_viewport() {
        for area in [Rect::new(0, 0, 80, 24), Rect::new(3, 4, 160, 48)] {
            let areas = spreadsheet_areas(area);
            for region in [
                areas.formula,
                areas.grid,
                areas.tabs,
                areas.controls,
                areas.status,
            ] {
                assert!(region.x >= area.x);
                assert!(region.y >= area.y);
                assert!(region.right() <= area.right());
                assert!(region.bottom() <= area.bottom());
            }
            let grid = GridGeometry::new(areas.grid, 1, 1);
            let last = grid
                .cell_area(grid.columns, grid.rows)
                .expect("last visible cell");
            assert!(last.right() <= area.right());
            assert!(last.bottom() <= area.bottom());
            assert!(grid.row_header_area(grid.rows).is_some());
        }
    }

    #[test]
    fn controls_wrap_whole_and_tabs_stop_at_the_edge() {
        let controls = pack_control_areas(Rect::new(0, 0, 80, 2));
        assert_eq!(controls.len(), SpreadsheetControl::ALL.len());
        assert!(controls.iter().all(|(_, area)| area.right() <= 80));
        assert!(controls.iter().any(|(_, area)| area.y == 1));

        let tabs = pack_tab_areas(Rect::new(2, 3, 14, 1), [(0, 3), (1, 8), (2, 8)]);
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[1].1.right(), 13);
    }
}
