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
}

impl fmt::Display for AddressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => write!(formatter, "expected a cell address such as A1"),
            Self::ColumnOutOfBounds(column) => write!(formatter, "column {column} is outside A:Z"),
            Self::RowOutOfBounds(row) => write!(formatter, "row {row} is outside 1:{MAX_ROWS}"),
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
        assert_eq!("AA1".parse::<CellAddress>(), Err(AddressError::InvalidFormat));
        assert_eq!("A0".parse::<CellAddress>(), Err(AddressError::RowOutOfBounds(0)));
        assert_eq!("Z101".parse::<CellAddress>(), Err(AddressError::RowOutOfBounds(101)));
    }

    #[test]
    fn ranges_are_normalized_and_iterate_by_row() {
        let range = CellRange::new("B2".parse().unwrap(), "A1".parse().unwrap());
        let addresses = range.addresses().map(|address| address.to_string()).collect::<Vec<_>>();
        assert_eq!(addresses, ["A1", "B1", "A2", "B2"]);
    }
}
