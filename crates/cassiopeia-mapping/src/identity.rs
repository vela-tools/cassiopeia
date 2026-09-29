use crate::template::{CompiledTemplate, TemplateSource};
use getset::{Getters, Setters};
use serde::{Deserialize, Serialize};

/// How a mapping derives an entity's `id` from a source record.
///
/// Only the `id` identifies an NGSI-LD entity (ETSI GS CIM 009 v1.9.1 clause 3.1 and Table 5.2.4-1),
/// so this block holds nothing but the settings that build it. Entity members that can change over
/// the entity's lifetime, such as `scope`, are declared on the [`Mapping`](crate::mapping::Mapping)
/// instead.
///
/// `deny_unknown_fields` enforces the v4 identity shape: a document declaring an unrecognized
/// field such as `urn` or `scope` is rejected outright rather than parsed and silently ignored.
#[derive(Debug, Clone, Serialize, Deserialize, Getters, Setters)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    /// The template producing the entity name, which becomes the trailing segment of its URN.
    #[getset(get = "pub")]
    entity_name: TemplateSource,

    /// The compiled `entity_name`, filled in once the mapping is loaded.
    #[serde(skip)]
    #[getset(get = "pub", set = "pub")]
    compiled_entity_name: Option<CompiledTemplate>,
}

impl Identity {
    /// Declares an identity from its entity-name template.
    #[must_use]
    pub const fn new(entity_name: TemplateSource) -> Identity {
        Identity {
            entity_name,
            compiled_entity_name: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{identity::Identity, template::TemplateSource};

    #[test]
    fn reads_an_entity_name() {
        let identity: Identity = serde_json::from_str(r#"{"entityName": "Station-{{ id }}"}"#).unwrap();

        assert_eq!(identity.entity_name(), &TemplateSource::new("Station-{{ id }}"));
    }

    #[test]
    fn a_scope_declared_inside_the_identity_is_rejected() {
        let result = serde_json::from_str::<Identity>(r#"{"entityName": "Station-{{ id }}", "scope": "/test"}"#);

        assert!(result.is_err());
    }

    #[test]
    fn the_superseded_urn_declaration_is_rejected() {
        let result = serde_json::from_str::<Identity>(r#"{"entityName": "Station-{{ id }}", "urn": "something"}"#);

        assert!(result.is_err());
    }

    #[test]
    fn an_entity_name_is_required() {
        assert!(serde_json::from_str::<Identity>("{}").is_err());
    }

    #[test]
    fn the_compiled_template_is_not_part_of_the_wire_form() {
        let identity = Identity::new(TemplateSource::new("Station-{{ id }}"));

        assert_eq!(serde_json::to_string(&identity).unwrap(), r#"{"entityName":"Station-{{ id }}"}"#);
    }
}
