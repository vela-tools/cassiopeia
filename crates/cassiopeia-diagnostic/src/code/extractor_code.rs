use strum::{Display, EnumCount, EnumIter};

/// Why an entity's attribute values could not be extracted from its source record.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum ExtractorCode {
    /// An attribute's template could not be evaluated against the record.
    TemplateUnresolvable,
    /// The mapping's nested attributes recursed past the guard depth.
    RecursionLimitExceeded,
    /// An attribute declared a temporal transformation and its source carried text that reads as no
    /// supported spelling of a date-time, so the attribute was dropped.
    TimestampUnreadable,
}

#[cfg(test)]
mod tests {
    use crate::code::extractor_code::ExtractorCode;

    #[test]
    fn an_extractor_code_renders_a_kebab_case_token() {
        assert_eq!(ExtractorCode::TemplateUnresolvable.to_string(), "template-unresolvable");
        assert_eq!(ExtractorCode::TimestampUnreadable.to_string(), "timestamp-unreadable");
    }
}
