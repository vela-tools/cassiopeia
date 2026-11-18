use derive_more::Display;
use serde::Deserialize;

/// A key exactly as a mapping document spells it, kept when no declaration reads it, so that
/// loading the mapping can name it.
#[derive(Clone, Debug, Deserialize, Display, Eq, Hash, PartialEq)]
#[serde(transparent)]
pub struct DocumentKey(String);

impl DocumentKey {
    /// The key as the document spells it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::document_key::DocumentKey;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_key_reads_and_renders_as_the_document_spells_it() {
        let key: DocumentKey = serde_json::from_str(r#""transformaton""#).unwrap();

        assert_eq!(key.as_str(), "transformaton");
        assert_eq!(key.to_string(), "transformaton");
    }
}
