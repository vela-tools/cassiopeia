use crate::entity_id::EntityId;
use derive_more::Display;

/// How a report names an Observation: by its id, or as one served without an id.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum ObservationName {
    /// The Observation's id.
    #[display("Observation {_0}")]
    Id(EntityId),
    /// The Observation was served without an `@iot.id`.
    #[display("an Observation without an id")]
    WithoutId,
}

#[cfg(test)]
mod tests {
    use crate::{entity_id::EntityId, observation_name::ObservationName};
    use pretty_assertions::assert_eq;

    #[test]
    fn an_observation_is_named_by_its_id_or_as_one_without_an_id() {
        assert_eq!(ObservationName::Id(EntityId::Integer(501)).to_string(), "Observation 501");
        assert_eq!(ObservationName::Id(EntityId::Text("o-7".into())).to_string(), "Observation o-7");
        assert_eq!(ObservationName::WithoutId.to_string(), "an Observation without an id");
    }
}
