use crate::sensorthings::error::{SensorThingsIngestError, StaEntity};
use cassiopeia_sensorthings::{entity_id::EntityId, snapshot::line_number::LineNumber};
use serde::Deserialize;
use serde_json::{Map, Value};

/// The member a served entity carries its id in (OGC 18-088 section 8.1, Table 2).
pub(super) const ID: &str = "@iot.id";

/// The id of a served `entity`, read from its `@iot.id` on `line`.
///
/// # Errors
///
/// Returns [`SensorThingsIngestError::IdUnreadable`] when the entity has no id, or one that is
/// neither an integer nor a text.
pub(super) fn served_id(served: &Map<String, Value>, line: LineNumber, entity: StaEntity) -> Result<EntityId, SensorThingsIngestError> {
    served
        .get(ID)
        .and_then(|id| EntityId::deserialize(id).ok())
        .ok_or(SensorThingsIngestError::IdUnreadable { line, entity })
}

/// The id of a served `entity` that may leave its id out, read from its `@iot.id` on `line`.
///
/// # Errors
///
/// Returns [`SensorThingsIngestError::IdUnreadable`] when the entity has an id that is neither an
/// integer nor a text.
pub(super) fn optional_served_id(served: &Map<String, Value>, line: LineNumber, entity: StaEntity) -> Result<Option<EntityId>, SensorThingsIngestError> {
    served.get(ID).map(|_| served_id(served, line, entity)).transpose()
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{
        error::{SensorThingsIngestError, StaEntity},
        served_id::{optional_served_id, served_id},
    };
    use cassiopeia_sensorthings::{entity_id::EntityId, snapshot::line_number::LineNumber};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

    #[test]
    fn an_id_keeps_its_json_type() {
        assert_eq!(
            served_id(&object(json!({"@iot.id": "t-1"})), LineNumber::FIRST, StaEntity::Thing).unwrap(),
            EntityId::Text("t-1".into())
        );
    }

    #[test]
    fn a_missing_or_unreadable_id_names_the_line_and_entity() {
        for served in [json!({}), json!({"@iot.id": 1.5}), json!({"@iot.id": null})] {
            let error = served_id(&object(served), LineNumber::FIRST, StaEntity::Datastream).unwrap_err();
            assert!(matches!(
                error,
                SensorThingsIngestError::IdUnreadable {
                    entity: StaEntity::Datastream,
                    ..
                }
            ));
        }
    }

    #[test]
    fn an_optional_id_may_be_left_out_but_not_unreadable() {
        assert_eq!(
            optional_served_id(&object(json!({})), LineNumber::FIRST, StaEntity::ObservedProperty).unwrap(),
            None
        );
        assert!(optional_served_id(&object(json!({"@iot.id": true})), LineNumber::FIRST, StaEntity::ObservedProperty).is_err());
    }
}
