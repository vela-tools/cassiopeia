use strum::{Display, EnumCount, EnumIter};

/// Why a source could not be brought into the pipeline as records, or was brought in with a shape
/// other than the one it states.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum IngestCode {
    /// Columns of a tabular source asked for a record key another column holds, so each was read
    /// by a generated key instead.
    ColumnsRenamed,
    /// Rows of a tabular source had another number of fields than the table, so each was skipped.
    RowsMisshapen,
    /// A table of a source holding several failed before its first row was read, so it was skipped
    /// and the other tables were read.
    TableUnreadable,
    /// The source could not be fetched or opened.
    SourceUnavailable,
    /// The payload's format could not be determined from its content.
    FormatUndetected,
    /// The detected format has no ingestor registered for it.
    Unroutable,
    /// A stage handoff closed before the payload or its records could be passed on.
    StreamClosed,
    /// The payload's records could not be read or parsed.
    RecordsUnreadable,
}

#[cfg(test)]
mod tests {
    use crate::code::ingest_code::IngestCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_ingest_code_renders_a_kebab_case_token() {
        assert_eq!(IngestCode::RecordsUnreadable.to_string(), "records-unreadable");
        assert_eq!(IngestCode::ColumnsRenamed.to_string(), "columns-renamed");
        assert_eq!(IngestCode::RowsMisshapen.to_string(), "rows-misshapen");
        assert_eq!(IngestCode::TableUnreadable.to_string(), "table-unreadable");
    }
}
