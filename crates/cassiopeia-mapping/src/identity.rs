use crate::{
    scope::{CompiledScope, Scope},
    template::{CompiledTemplate, TemplateSource},
};
use getset::{Getters, Setters};
use serde::{Deserialize, Serialize};

/// How a mapping derives an entity's identity from a source record.
///
/// `deny_unknown_fields` enforces the v4 identity shape: a document declaring an unrecognized
/// field such as `urn` is rejected outright rather than parsed and silently ignored.
#[derive(Debug, Clone, Serialize, Deserialize, Getters, Setters)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    /// The template producing the entity name, which becomes the trailing segment of its URN.
    #[getset(get = "pub")]
    entity_name: TemplateSource,

    /// The optional scope declaration.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[getset(get = "pub")]
    scope: Option<Scope>,

    /// The compiled `entity_name`, filled in once the mapping is loaded.
    #[serde(skip)]
    #[getset(get = "pub", set = "pub")]
    compiled_entity_name: Option<CompiledTemplate>,

    /// The compiled `scope`, filled in once the mapping is loaded.
    #[serde(skip)]
    #[getset(get = "pub", set = "pub")]
    compiled_scope: Option<CompiledScope>,
}

impl Identity {
    /// Declares an identity from its entity-name template and optional scope.
    #[must_use]
    pub const fn new(entity_name: TemplateSource, scope: Option<Scope>) -> Identity {
        Identity {
            entity_name,
            scope,
            compiled_entity_name: None,
            compiled_scope: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{identity::Identity, scope::Scope, template::TemplateSource};

    #[test]
    fn reads_an_entity_name_and_scope() {
        let identity: Identity = serde_json::from_str(r#"{"entityName": "Station-{{ id }}", "scope": "/test"}"#).unwrap();

        assert_eq!(identity.entity_name(), &TemplateSource::new("Station-{{ id }}"));
        assert_eq!(identity.scope(), &Some(Scope::Single(TemplateSource::new("/test"))));
    }

    #[test]
    fn scope_is_optional() {
        let identity: Identity = serde_json::from_str(r#"{"entityName": "Station-{{ id }}"}"#).unwrap();

        assert_eq!(identity.scope(), &None);
    }

    #[test]
    fn the_superseded_urn_declaration_is_rejected() {
        let result = serde_json::from_str::<Identity>(r#"{"entityName": "Station-{{ id }}", "urn": "something"}"#);

        assert!(result.is_err());
    }

    #[test]
    fn an_entity_name_is_required() {
        assert!(serde_json::from_str::<Identity>(r#"{"scope": "/test"}"#).is_err());
    }

    #[test]
    fn the_compiled_templates_are_not_part_of_the_wire_form() {
        let identity = Identity::new(TemplateSource::new("Station-{{ id }}"), None);

        assert_eq!(serde_json::to_string(&identity).unwrap(), r#"{"entityName":"Station-{{ id }}"}"#);
    }
}
