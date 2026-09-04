use strum::{Display, EnumCount, EnumIter};

/// Why the `@context` a run attaches to its entities could not be resolved or delivered as asked.
///
/// None of these abort a run: an entity without an `@context` is still an entity, so every one of
/// them degrades rather than fails.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum ContextCode {
    /// A mapping's data model is not a valid Smart Data Models identifier, so no context URL can be
    /// looked up for it.
    ModelIdInvalid,
    /// The schema store the context URL would be looked up in could not be opened.
    StoreUnavailable,
    /// The catalog lookup for a model's context URL failed.
    LookupFailed,
    /// A local `@context` file could not be read.
    FileUnreadable,
    /// A local `@context` file is not valid JSON.
    FileUnparsable,
    /// A local `@context` file's `@context` member is not a shape NGSI-LD admits.
    ContextUnreadable,
    /// The context URL is not a legal HTTP header value, so link-header delivery fell back to body
    /// delivery.
    LinkHeaderInvalid,
    /// Link-header delivery was asked for but the context is not a single URL, so it fell back to
    /// body delivery.
    LinkDeliveryUnsupported,
}

#[cfg(test)]
mod tests {
    use crate::code::context_code::ContextCode;

    #[test]
    fn a_context_code_renders_a_kebab_case_token() {
        assert_eq!(ContextCode::LinkDeliveryUnsupported.to_string(), "link-delivery-unsupported");
    }
}
