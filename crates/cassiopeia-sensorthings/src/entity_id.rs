use derive_more::Display;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The `@iot.id` of a SensorThings entity (OGC 18-088 section 8.1, Table 2).
///
/// A server chooses the type of its ids: RIVM Samen Meten sends integers, and a FROST-Server set up
/// for them sends texts. An id keeps the JSON type it was sent with, so it is written back exactly as
/// the server reads it; any other JSON type is no id.
///
/// Ids are ordered integers first, integers by value and texts by their bytes. That order decides
/// which Location of a Thing is read and which data stream keeps a repeated reading name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Display, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EntityId {
    /// An integer id, such as RIVM's `56776`.
    #[display("{_0}")]
    Integer(i64),

    /// A text id, such as `"t-1"`.
    #[display("{_0}")]
    Text(Box<str>),
}

impl EntityId {
    /// The id as the JSON value it was sent as.
    #[must_use]
    pub fn json(&self) -> Value {
        match self {
            EntityId::Integer(id) => Value::from(*id),
            EntityId::Text(id) => Value::from(id.as_ref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_id::EntityId;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use serde_json::json;

    #[test]
    fn entity_ids_keep_their_json_type() {
        assert_eq!(EntityId::deserialize(&json!(56776)).unwrap(), EntityId::Integer(56776));
        assert_eq!(EntityId::deserialize(&json!("abc")).unwrap(), EntityId::Text("abc".into()));
        assert!(EntityId::deserialize(&json!(1.5)).is_err());
        assert!(EntityId::deserialize(&json!(true)).is_err());
    }

    #[test]
    fn integer_ids_sort_before_text_ids() {
        let mut ids = vec![EntityId::Integer(10), EntityId::Text("a".into()), EntityId::Integer(2)];
        ids.sort();

        assert_eq!(ids, vec![EntityId::Integer(2), EntityId::Integer(10), EntityId::Text("a".into())]);
    }

    #[test]
    fn an_id_is_displayed_bare_and_written_back_as_its_json_type() {
        assert_eq!(EntityId::Integer(12512).to_string(), "12512");
        assert_eq!(EntityId::Text("t-1".into()).to_string(), "t-1");
        assert_eq!(EntityId::Integer(12512).json(), json!(12512));
        assert_eq!(EntityId::Text("t-1".into()).json(), json!("t-1"));
    }
}
