use crate::{attribute::Attributes, template::CompiledTemplate};
use getset::{Getters, MutGetters, Setters};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use typed_builder::TypedBuilder;

/// One instance of a multi-instance attribute (ETSI GS CIM 009 v1.9.1 clause 4.5.5).
///
/// An attribute name may carry several instances that differ by `datasetId`; each instance reads its
/// own value `source` and declares its own `properties` (notably `datasetId`). The attribute's
/// `type`, `transformation`, and attribute-level `properties` are shared across every instance, so an
/// instance only restates what makes it distinct. An instance without a `datasetId` is the default
/// instance, of which the spec permits at most one.
#[derive(Debug, Clone, Serialize, Deserialize, Getters, MutGetters, Setters, TypedBuilder)]
pub struct AttributeInstance {
    /// The template, or list of templates, producing this instance's value.
    ///
    /// What the resolved text denotes follows the attribute's kind: a value for a Property instance,
    /// the target object-id for a Relationship instance, and a tokenized object-id list for a
    /// `ListRelationship` instance (ETSI GS CIM 009 v1.9.1 clause 4.5.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    #[getset(get = "pub")]
    source: Option<JsonValue>,

    /// This instance's own properties, such as its `datasetId`.
    ///
    /// Mutable access exists so the expansion stage can compile property templates in place. These
    /// merge over the attribute's shared properties, so an instance may also override a shared
    /// qualifier such as `observedAt`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    #[getset(get = "pub", get_mut = "pub")]
    properties: Option<Attributes>,

    /// The compiled `source` templates, filled in once the mapping is loaded.
    #[serde(skip)]
    #[builder(default = Default::default())]
    #[getset(get = "pub", set = "pub")]
    compiled_source: Option<Vec<CompiledTemplate>>,
}

#[cfg(test)]
mod tests {
    use crate::attribute::Attribute;
    use cassiopeia_ngsi_ld::entity::name::NameBuf;

    fn parse(document: &str) -> Attribute {
        serde_json5::from_str(document).unwrap()
    }

    #[test]
    fn an_attribute_reads_its_instances() {
        let attribute = parse(
            r#"{
                "type": "Property",
                "transformation": "float",
                "instances": [
                    {"source": "{{ a }}", "properties": {"datasetId": {"source": "urn:ngsi-ld:dataset:model:a"}}},
                    {"source": "{{ b }}", "properties": {"datasetId": {"source": "urn:ngsi-ld:dataset:model:b"}}}
                ]
            }"#,
        );

        let instances = attribute.instances().as_ref().expect("instances are present");
        assert_eq!(instances.len(), 2);
    }

    #[test]
    fn an_instance_carries_its_dataset_id_property() {
        let attribute =
            parse(r#"{"type": "Property", "instances": [{"source": "{{ a }}", "properties": {"datasetId": {"source": "urn:ngsi-ld:dataset:model:a"}}}]}"#);

        let instances = attribute.instances().as_ref().expect("instances are present");
        let dataset_id = NameBuf::new("datasetId").unwrap();
        assert!(instances[0].properties().as_ref().unwrap().contains_key(&dataset_id));
    }

    #[test]
    fn an_attribute_without_instances_has_none() {
        assert!(parse(r#"{"source": "{{ a }}"}"#).instances().is_none());
    }
}
