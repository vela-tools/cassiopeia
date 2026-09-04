use derive_more::{Display, From, Into};
use serde::{Deserialize, Serialize};

/// The label of one logical record collection inside a multi-collection source.
///
/// A single file can pack several distinct record collections: a KML `<Document>` with sibling
/// `<Folder>`s, or (later) an Excel workbook with several sheets. Each such collection carries a
/// name that the manifest routes on. That name is kept **verbatim**: collection labels are arbitrary
/// Unicode: folder and sheet names allow spaces, punctuation, CJK, and emoji (Excel forbids only
/// `\ / ? * [ ] :`), none of which the NGSI-LD name grammar permits. Routing is therefore exact
/// string equality on the raw label, never a grammar-validated
/// [`NameBuf`](cassiopeia_ngsi_ld::entity::name::NameBuf).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, From, Into)]
#[serde(transparent)]
pub struct CollectionName(String);

impl CollectionName {
    /// Returns the collection label as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CollectionName {
    fn from(value: &str) -> CollectionName {
        CollectionName(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::collection::CollectionName;

    #[test]
    fn a_collection_name_round_trips_arbitrary_unicode_verbatim() {
        for label in ["Camera Area", "Café ☕", "監視カメラ"] {
            let name = CollectionName::from(label);
            assert_eq!(name.as_str(), label);

            let json = serde_json::to_string(&name).unwrap();
            let restored: CollectionName = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, name);
        }
    }

    #[test]
    fn differently_cased_labels_are_unequal() {
        assert_ne!(CollectionName::from("Camera"), CollectionName::from("camera"));
    }

    #[test]
    fn a_label_serializes_transparently_as_a_bare_string() {
        assert_eq!(serde_json::to_string(&CollectionName::from("Flowcount")).unwrap(), r#""Flowcount""#);
    }
}
