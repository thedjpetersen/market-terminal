use std::{fmt, str::FromStr};

pub const MAX_COLUMNS: u8 = 26;
pub const MAX_ROWS: u16 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellAddress {
    column: u8,
    row: u16,
}

impl CellAddress {
    pub fn new(column: u8, row: u16) -> Result<Self, AddressError> {
        if !(1..=MAX_COLUMNS).contains(&column) {
            return Err(AddressError::ColumnOutOfBounds(column));
        }
        if !(1..=MAX_ROWS).contains(&row) {
            return Err(AddressError::RowOutOfBounds(row));
        }
        Ok(Self { column, row })
    }

    pub const fn column(self) -> u8 {
        self.column
    }

    pub const fn row(self) -> u16 {
        self.row
    }
}

impl FromStr for CellAddress {
    type Err = AddressError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        let mut chars = input.chars().peekable();
        if chars.peek() == Some(&'$') {
            chars.next();
        }
        let column = chars.next().ok_or(AddressError::InvalidFormat)?;
        if !column.is_ascii_alphabetic() {
            return Err(AddressError::InvalidFormat);
        }
        if chars.peek().is_some_and(|next| next.is_ascii_alphabetic()) {
            return Err(AddressError::InvalidFormat);
        }
        if chars.peek() == Some(&'$') {
            chars.next();
        }
        let row_text: String = chars.collect();
        if row_text.is_empty() || !row_text.chars().all(|character| character.is_ascii_digit()) {
            return Err(AddressError::InvalidFormat);
        }
        let column = column.to_ascii_uppercase() as u8 - b'A' + 1;
        let row = row_text.parse().map_err(|_| AddressError::InvalidFormat)?;
        Self::new(column, row)
    }
}

impl fmt::Display for CellAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let column = char::from(b'A' + self.column - 1);
        write!(formatter, "{column}{}", self.row)
    }
}

/// A formula reference preserves relative/absolute axes independently from the
/// address it resolves to. This metadata is what makes copy and fill
/// translation deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellReference {
    address: CellAddress,
    absolute_column: bool,
    absolute_row: bool,
}

impl CellReference {
    pub const fn new(address: CellAddress, absolute_column: bool, absolute_row: bool) -> Self {
        Self {
            address,
            absolute_column,
            absolute_row,
        }
    }

    pub const fn address(self) -> CellAddress {
        self.address
    }

    pub const fn absolute_column(self) -> bool {
        self.absolute_column
    }

    pub const fn absolute_row(self) -> bool {
        self.absolute_row
    }

    pub fn translated(self, column_delta: i16, row_delta: i32) -> Result<Self, AddressError> {
        let column = if self.absolute_column {
            i16::from(self.address.column())
        } else {
            i16::from(self.address.column()) + column_delta
        };
        let row = if self.absolute_row {
            i32::from(self.address.row())
        } else {
            i32::from(self.address.row()) + row_delta
        };
        if !(1..=i16::from(MAX_COLUMNS)).contains(&column)
            || !(1..=i32::from(MAX_ROWS)).contains(&row)
        {
            return Err(AddressError::TranslationOutOfBounds);
        }
        let address = CellAddress::new(
            u8::try_from(column).expect("translated column was bounded"),
            u16::try_from(row).expect("translated row was bounded"),
        )?;
        Ok(Self { address, ..self })
    }
}

impl FromStr for CellReference {
    type Err = AddressError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        let absolute_column = input.starts_with('$');
        let without_column_marker = input.strip_prefix('$').unwrap_or(input);
        let Some(row_marker) = without_column_marker.find('$') else {
            return Ok(Self::new(input.parse()?, absolute_column, false));
        };
        if row_marker == 0 || without_column_marker[row_marker + 1..].contains('$') {
            return Err(AddressError::InvalidFormat);
        }
        Ok(Self::new(input.parse()?, absolute_column, true))
    }
}

impl fmt::Display for CellReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let column = char::from(b'A' + self.address.column - 1);
        write!(
            formatter,
            "{}{}{}{}",
            if self.absolute_column { "$" } else { "" },
            column,
            if self.absolute_row { "$" } else { "" },
            self.address.row,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRange {
    start: CellAddress,
    end: CellAddress,
}

impl CellRange {
    pub fn new(first: CellAddress, second: CellAddress) -> Self {
        let start = CellAddress {
            column: first.column.min(second.column),
            row: first.row.min(second.row),
        };
        let end = CellAddress {
            column: first.column.max(second.column),
            row: first.row.max(second.row),
        };
        Self { start, end }
    }

    pub const fn start(self) -> CellAddress {
        self.start
    }

    pub const fn end(self) -> CellAddress {
        self.end
    }

    pub fn addresses(self) -> impl Iterator<Item = CellAddress> {
        (self.start.row..=self.end.row).flat_map(move |row| {
            (self.start.column..=self.end.column).map(move |column| CellAddress { column, row })
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressError {
    InvalidFormat,
    ColumnOutOfBounds(u8),
    RowOutOfBounds(u16),
    TranslationOutOfBounds,
}

impl fmt::Display for AddressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => write!(formatter, "expected a cell address such as A1"),
            Self::ColumnOutOfBounds(column) => write!(formatter, "column {column} is outside A:Z"),
            Self::RowOutOfBounds(row) => write!(formatter, "row {row} is outside 1:{MAX_ROWS}"),
            Self::TranslationOutOfBounds => {
                write!(formatter, "translated reference is outside A1:Z{MAX_ROWS}")
            }
        }
    }
}

impl std::error::Error for AddressError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_displays_addresses_canonically() {
        let address: CellAddress = "$c$42".parse().unwrap();
        assert_eq!(address.column(), 3);
        assert_eq!(address.row(), 42);
        assert_eq!(address.to_string(), "C42");
    }

    #[test]
    fn rejects_addresses_outside_the_sheet() {
        assert_eq!(
            "AA1".parse::<CellAddress>(),
            Err(AddressError::InvalidFormat)
        );
        assert_eq!(
            "A0".parse::<CellAddress>(),
            Err(AddressError::RowOutOfBounds(0))
        );
        assert_eq!(
            "Z101".parse::<CellAddress>(),
            Err(AddressError::RowOutOfBounds(101))
        );
    }

    #[test]
    fn ranges_are_normalized_and_iterate_by_row() {
        let range = CellRange::new("B2".parse().unwrap(), "A1".parse().unwrap());
        let addresses = range
            .addresses()
            .map(|address| address.to_string())
            .collect::<Vec<_>>();
        assert_eq!(addresses, ["A1", "B1", "A2", "B2"]);
    }

    #[test]
    fn references_preserve_absolute_axes_during_translation() {
        let relative: CellReference = "B2".parse().unwrap();
        let mixed: CellReference = "$B2".parse().unwrap();
        let absolute: CellReference = "$B$2".parse().unwrap();
        assert_eq!(relative.translated(2, 3).unwrap().to_string(), "D5");
        assert_eq!(mixed.translated(2, 3).unwrap().to_string(), "$B5");
        assert_eq!(absolute.translated(2, 3).unwrap().to_string(), "$B$2");
        assert_eq!(
            relative.translated(-2, 0),
            Err(AddressError::TranslationOutOfBounds)
        );
    }
}
