use crate::unread_keys::UnreadKeys;
use cassiopeia_ngsi_ld::data_model::DataModel;
use getset::{Getters, MutGetters};
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// The entity a relationship attribute points at.
///
/// A relationship's object is another entity's URN, minted from the attribute's `source` under the
/// data model the target names.
#[derive(Debug, Clone, Serialize, Deserialize, Getters, MutGetters, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// The data model of the entity being pointed at.
    #[getset(get = "pub")]
    entity: DataModel,

    /// The keys this target holds that it does not read, taken out when the mapping loads so each
    /// can be warned about.
    #[serde(flatten, skip_serializing)]
    #[builder(default = Default::default(), setter(skip))]
    #[getset(get_mut = "pub(crate)")]
    unread_keys: UnreadKeys,
}

#[cfg(test)]
mod tests {
    use crate::target::Target;
    use pretty_assertions::assert_eq;

    #[test]
    fn reads_a_target_naming_only_its_entity() {
        let mut target: Target = serde_json::from_str(r#"{"entity": "dataModel.Transportation/Road"}"#).unwrap();

        assert_eq!(target.entity().entity_type().as_str(), "Road");
        assert_eq!(target.unread_keys_mut().take().count(), 0);
    }

    #[test]
    fn a_target_keeps_the_keys_it_does_not_read() {
        let mut target: Target = serde_json::from_str(r#"{"entity": "Road", "sourceField": "road_id", "idPattern": "Road-{{ road_id }}"}"#).unwrap();

        assert_eq!(target.entity().entity_type().as_str(), "Road");
        assert_eq!(
            target.unread_keys_mut().take().map(|key| key.to_string()).collect::<Vec<_>>(),
            ["sourceField", "idPattern"]
        );
    }

    #[test]
    fn a_target_without_an_entity_is_rejected() {
        assert!(serde_json::from_str::<Target>(r#"{"sourceField": "road_id"}"#).is_err());
    }

    #[test]
    fn an_entity_that_is_not_a_valid_data_model_is_rejected() {
        assert!(serde_json::from_str::<Target>(r#"{"entity": "9Road"}"#).is_err());
    }
}
