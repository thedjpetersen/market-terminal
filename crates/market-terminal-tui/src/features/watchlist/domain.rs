use crate::foundation::InstrumentId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorColumn {
    Symbol,
    Last,
    Change,
    ChangePercent,
    Bid,
    Ask,
    Volume,
    DayRange,
    Sparkline,
    Quality,
    AsOf,
}

impl MonitorColumn {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Symbol => "SYMBOL",
            Self::Last => "LAST",
            Self::Change => "CHG",
            Self::ChangePercent => "% CHG",
            Self::Bid => "BID",
            Self::Ask => "ASK",
            Self::Volume => "VOLUME",
            Self::DayRange => "DAY RANGE",
            Self::Sparkline => "SESSION",
            Self::Quality => "QUALITY",
            Self::AsOf => "AS OF",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Symbol,
    Last,
    ChangePercent,
    Volume,
}

impl SortField {
    pub const fn next(self) -> Self {
        match self {
            Self::Symbol => Self::Last,
            Self::Last => Self::ChangePercent,
            Self::ChangePercent => Self::Volume,
            Self::Volume => Self::Symbol,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Symbol => "SYMBOL",
            Self::Last => "LAST",
            Self::ChangePercent => "% CHG",
            Self::Volume => "VOLUME",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub const fn toggled(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }

    pub const fn marker(self) -> &'static str {
        match self {
            Self::Ascending => "ASC",
            Self::Descending => "DESC",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortSpec {
    pub field: SortField,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchlistItem {
    pub instrument_id: InstrumentId,
    pub symbol: String,
    pub description: String,
}

impl WatchlistItem {
    pub fn new(
        instrument_id: InstrumentId,
        symbol: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            instrument_id,
            symbol: symbol.into(),
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchlistDefinition {
    pub id: String,
    pub name: String,
    pub items: Vec<WatchlistItem>,
    pub visible_columns: Vec<MonitorColumn>,
    pub sort: SortSpec,
}

impl WatchlistDefinition {
    pub fn new(id: impl Into<String>, name: impl Into<String>, items: Vec<WatchlistItem>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            items,
            visible_columns: Self::full_columns(),
            sort: SortSpec {
                field: SortField::Symbol,
                direction: SortDirection::Ascending,
            },
        }
    }

    pub fn with_columns(mut self, columns: Vec<MonitorColumn>) -> Self {
        assert!(
            columns.contains(&MonitorColumn::Symbol),
            "monitor must show SYMBOL"
        );
        self.visible_columns = columns;
        self
    }

    pub fn with_sort(mut self, sort: SortSpec) -> Self {
        self.sort = sort;
        self
    }

    pub fn full_columns() -> Vec<MonitorColumn> {
        vec![
            MonitorColumn::Symbol,
            MonitorColumn::Last,
            MonitorColumn::Change,
            MonitorColumn::ChangePercent,
            MonitorColumn::Bid,
            MonitorColumn::Ask,
            MonitorColumn::Volume,
            MonitorColumn::DayRange,
            MonitorColumn::Sparkline,
            MonitorColumn::Quality,
            MonitorColumn::AsOf,
        ]
    }

    pub fn trading_columns() -> Vec<MonitorColumn> {
        vec![
            MonitorColumn::Symbol,
            MonitorColumn::Last,
            MonitorColumn::Change,
            MonitorColumn::ChangePercent,
            MonitorColumn::DayRange,
            MonitorColumn::Sparkline,
            MonitorColumn::Bid,
            MonitorColumn::Ask,
            MonitorColumn::Volume,
        ]
    }

    pub fn compact_columns() -> Vec<MonitorColumn> {
        vec![
            MonitorColumn::Symbol,
            MonitorColumn::Last,
            MonitorColumn::ChangePercent,
            MonitorColumn::Sparkline,
            MonitorColumn::Quality,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configurable_columns_always_retain_symbol_identity() {
        let definition = WatchlistDefinition::new("core", "Core", Vec::new()).with_columns(vec![
            MonitorColumn::Symbol,
            MonitorColumn::Last,
            MonitorColumn::Quality,
        ]);

        assert_eq!(definition.visible_columns.len(), 3);
        assert_eq!(definition.visible_columns[0], MonitorColumn::Symbol);
    }

    #[test]
    fn sort_fields_cycle_deterministically() {
        assert_eq!(SortField::Symbol.next(), SortField::Last);
        assert_eq!(SortField::Volume.next(), SortField::Symbol);
    }
}
