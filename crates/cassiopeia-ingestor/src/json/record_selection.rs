use cassiopeia_common::collection::CollectionName;

/// How the JSON ingestor picks the records out of a document.
///
/// The choice follows the input's mapping binding: a lone `mapping` keeps the lone-array convention,
/// while a `mappings` list reads the document as nested collections and asks for the listed ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonRecordSelection {
    /// The document is one collection, split under the lone-array convention.
    LoneArray,
    /// Every non-empty array of objects is a collection labelled by its member path; only the
    /// collections with one of these labels yield records.
    Collections(Vec<CollectionName>),
}
