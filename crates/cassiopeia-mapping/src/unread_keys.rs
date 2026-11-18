use crate::document_key::DocumentKey;
use indexmap::IndexMap;
use serde::{Deserialize, de::IgnoredAny};
use std::mem;

/// The keys one declaration holds that it does not read, in the order the document spells them.
///
/// A declaration flattens this beside its own fields, so a key that names none of them lands here
/// instead of being dropped, and loading the mapping warns about it. Each value is skipped as it is
/// read: nothing reads a key no declaration knows, so its value is never needed.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(transparent)]
pub(crate) struct UnreadKeys(IndexMap<DocumentKey, IgnoredAny>);

impl UnreadKeys {
    /// Takes every key out, in document order, leaving none behind.
    pub(crate) fn take(&mut self) -> impl Iterator<Item = DocumentKey> + use<> {
        mem::take(&mut self.0).into_keys()
    }
}

#[cfg(test)]
mod tests {
    use crate::unread_keys::UnreadKeys;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_keys_are_taken_in_document_order_whatever_their_values() {
        let mut keys: UnreadKeys = serde_json5::from_str(r#"{ transformaton: "float", unitCode: { nested: [1, 2] }, Source: null }"#).unwrap();

        assert_eq!(
            keys.take().map(|key| key.to_string()).collect::<Vec<_>>(),
            ["transformaton", "unitCode", "Source"]
        );
        assert_eq!(keys.take().count(), 0);
    }
}
