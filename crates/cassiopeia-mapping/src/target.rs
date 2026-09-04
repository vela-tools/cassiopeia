use crate::template::{CompiledTemplate, TemplateSource};
use cassiopeia_ngsi_ld::data_model::DataModel;
use getset::{Getters, Setters};
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// The entity a relationship attribute points at.
///
/// A relationship's object is another entity's URN, so the mapping has to say which model that
/// entity belongs to and how to reconstruct its identity from the current record.
#[derive(Debug, Clone, Serialize, Deserialize, Getters, Setters, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// The data model of the entity being pointed at.
    #[getset(get = "pub")]
    entity: DataModel,

    /// The source field holding the target's identifier, when it is not derived from `id_pattern`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    #[getset(get = "pub")]
    source_field: Option<TemplateSource>,

    /// The template producing the target's entity name, when it is not a plain field value.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    #[getset(get = "pub")]
    id_pattern: Option<TemplateSource>,

    /// The compiled `id_pattern`, filled in once the mapping is loaded.
    #[serde(skip)]
    #[builder(default = Default::default())]
    #[getset(get = "pub", set = "pub")]
    compiled_id_pattern: Option<CompiledTemplate>,
}

#[cfg(test)]
mod tests {
    use crate::{target::Target, template::TemplateSource};

    #[test]
    fn reads_a_target_naming_only_its_entity() {
        let target: Target = serde_json::from_str(r#"{"entity": "dataModel.Transportation/Road"}"#).unwrap();

        assert_eq!(target.entity().entity_type().as_str(), "Road");
        assert_eq!(target.source_field(), &None);
    }

    #[test]
    fn reads_a_target_with_a_source_field_and_id_pattern() {
        let target: Target = serde_json::from_str(r#"{"entity": "Road", "sourceField": "road_id", "idPattern": "Road-{{ road_id }}"}"#).unwrap();

        assert_eq!(target.source_field(), &Some(TemplateSource::new("road_id")));
        assert_eq!(target.id_pattern(), &Some(TemplateSource::new("Road-{{ road_id }}")));
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
