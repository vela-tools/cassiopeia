use derive_more::Display;

/// The 1-based number of a line of a snapshot file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display)]
pub struct LineNumber(u64);

impl LineNumber {
    /// The header's line.
    pub const FIRST: LineNumber = LineNumber(1);

    /// The line after this one.
    #[must_use]
    pub const fn next(self) -> LineNumber {
        LineNumber(self.0.saturating_add(1))
    }

    /// The number, counted from 1.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::snapshot::line_number::LineNumber;
    use pretty_assertions::assert_eq;

    #[test]
    fn lines_are_counted_from_one() {
        assert_eq!(LineNumber::FIRST.get(), 1);
        assert_eq!(LineNumber::FIRST.next().next().to_string(), "3");
    }
}
