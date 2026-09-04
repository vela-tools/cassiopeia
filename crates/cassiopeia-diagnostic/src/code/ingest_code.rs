use strum::{Display, EnumCount, EnumIter};

/// Why a source could not be brought into the pipeline as records.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum IngestCode {
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

    #[test]
    fn an_ingest_code_renders_a_kebab_case_token() {
        assert_eq!(IngestCode::RecordsUnreadable.to_string(), "records-unreadable");
    }
}
