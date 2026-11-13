use derive_more::Display;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The member a served entity carries its id in (OGC 18-088 section 8.1, Table 2).
pub const ID_MEMBER: &str = "@iot.id";

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
    /// The id `served` carries in its `@iot.id`, or `None` when it carries none, or one that is
    /// neither an integer nor a text.
    #[must_use]
    pub fn of(served: &Map<String, Value>) -> Option<EntityId> {
        served.get(ID_MEMBER).and_then(|id| EntityId::deserialize(id).ok())
    }

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
    use serde_json::{Map, Value, json};

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

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

    #[test]
    fn a_served_entity_s_id_is_read_from_its_iot_id() {
        assert_eq!(EntityId::of(&object(json!({"@iot.id": 7, "name": "x"}))), Some(EntityId::Integer(7)));
        assert_eq!(EntityId::of(&object(json!({"@iot.id": "t-1"}))), Some(EntityId::Text("t-1".into())));
        assert_eq!(EntityId::of(&object(json!({"name": "x"}))), None);
        assert_eq!(EntityId::of(&object(json!({"@iot.id": null}))), None);
    }
}
