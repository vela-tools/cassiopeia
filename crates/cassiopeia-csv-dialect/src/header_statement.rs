use crate::{error::DialectError, header_flag::HeaderFlag, header_rows::HeaderRows};

/// What a manifest states about a CSV table's header, from its `header` and `headerRows` members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderStatement {
    /// The table has a header, of as many records as detection finds (`header: true`).
    Present,
    /// The table's header is exactly this many records; none for `header: false`.
    Rows(HeaderRows),
}

impl HeaderStatement {
    /// Resolves the two manifest members into one statement, or `None` when neither is stated.
    ///
    /// # Errors
    ///
    /// Returns [`DialectError::HeaderAbsentWithRows`] for `header: false` with one or more header
    /// rows, and [`DialectError::HeaderPresentWithoutRows`] for `header: true` with none.
    pub(crate) const fn from_members(header: Option<HeaderFlag>, header_rows: Option<HeaderRows>) -> Result<Option<HeaderStatement>, DialectError> {
        match (header, header_rows) {
            (None, None) => Ok(None),
            (Some(HeaderFlag::Present), None) => Ok(Some(HeaderStatement::Present)),
            (Some(HeaderFlag::Absent), None) => Ok(Some(HeaderStatement::Rows(HeaderRows::NONE))),
            (Some(HeaderFlag::Present), Some(rows)) if !rows.is_present() => Err(DialectError::HeaderPresentWithoutRows),
            (Some(HeaderFlag::Absent), Some(rows)) if rows.is_present() => Err(DialectError::HeaderAbsentWithRows(rows.records())),
            (None | Some(HeaderFlag::Present | HeaderFlag::Absent), Some(rows)) => Ok(Some(HeaderStatement::Rows(rows))),
        }
    }

    /// The manifest members that state this header: `header: true`, `header: false`, or
    /// `headerRows` alone for a header of one or more records.
    #[must_use]
    pub(crate) const fn members(self) -> (Option<HeaderFlag>, Option<HeaderRows>) {
        match self {
            HeaderStatement::Present => (Some(HeaderFlag::Present), None),
            HeaderStatement::Rows(rows) if rows.is_present() => (None, Some(rows)),
            HeaderStatement::Rows(_) => (Some(HeaderFlag::Absent), None),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::DialectError, header_flag::HeaderFlag, header_rows::HeaderRows, header_statement::HeaderStatement};
    use pretty_assertions::assert_eq;

    #[test]
    fn neither_member_states_nothing() {
        assert_eq!(HeaderStatement::from_members(None, None), Ok(None));
    }

    #[test]
    fn a_header_flag_alone_states_presence_or_absence() {
        assert_eq!(
            HeaderStatement::from_members(Some(HeaderFlag::Present), None),
            Ok(Some(HeaderStatement::Present))
        );
        assert_eq!(
            HeaderStatement::from_members(Some(HeaderFlag::Absent), None),
            Ok(Some(HeaderStatement::Rows(HeaderRows::NONE)))
        );
    }

    #[test]
    fn a_row_count_states_the_header_exactly_with_or_without_an_agreeing_flag() {
        let three = Ok(Some(HeaderStatement::Rows(HeaderRows::new(3))));

        assert_eq!(HeaderStatement::from_members(None, Some(HeaderRows::new(3))), three);
        assert_eq!(HeaderStatement::from_members(Some(HeaderFlag::Present), Some(HeaderRows::new(3))), three);
        assert_eq!(
            HeaderStatement::from_members(Some(HeaderFlag::Absent), Some(HeaderRows::NONE)),
            Ok(Some(HeaderStatement::Rows(HeaderRows::NONE)))
        );
    }

    #[test]
    fn a_flag_that_contradicts_the_row_count_is_rejected() {
        assert_eq!(
            HeaderStatement::from_members(Some(HeaderFlag::Absent), Some(HeaderRows::new(2))),
            Err(DialectError::HeaderAbsentWithRows(2))
        );
        assert_eq!(
            HeaderStatement::from_members(Some(HeaderFlag::Present), Some(HeaderRows::NONE)),
            Err(DialectError::HeaderPresentWithoutRows)
        );
    }

    #[test]
    fn a_statement_is_written_back_as_the_fewest_members_that_state_it() {
        assert_eq!(HeaderStatement::Present.members(), (Some(HeaderFlag::Present), None));
        assert_eq!(HeaderStatement::Rows(HeaderRows::NONE).members(), (Some(HeaderFlag::Absent), None));
        assert_eq!(HeaderStatement::Rows(HeaderRows::new(3)).members(), (None, Some(HeaderRows::new(3))));
    }
}
