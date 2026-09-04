pub mod attribute;
pub mod builder;
pub mod context;
pub mod error;
pub mod name;
pub mod representation;
pub mod reserved_member;
pub mod scope;
pub mod temporal_aggregate;
#[cfg(test)]
mod tests;
pub mod validation;

use crate::entity::{
    attribute::Attributes,
    context::NgsiLdContext,
    error::Result as NgsiLdResult,
    name::NameBuf,
    representation::{DisplayStr, JsonLayout, NgsiLdSerializable, ReprAdapter, SerializeRepr},
    scope::NgsiLdScope,
};
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap};
use serde_json::Value as JsonValue;
use urn_rs::Urn;

/// A single NGSI-LD entity (ETSI GS CIM 009 v1.9.1, clause 4.5.1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NgsiLdEntity {
    /// The JSON-LD `@context`, when one is attached.
    #[serde(rename = "@context", skip_serializing_if = "Option::is_none")]
    pub context: Option<NgsiLdContext>,
    /// The entity identifier (a URN).
    pub id: Urn,
    /// The entity type name.
    #[serde(rename = "type")]
    pub entity_type: NameBuf,
    /// The Smart Data Model this entity was produced from.
    ///
    /// Carried through the pipeline for schema resolution but never serialized.
    #[serde(skip)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<NgsiLdScope>,
    /// The entity's attributes, flattened into the top-level object on serialization.
    #[serde(flatten)]
    pub attributes: Attributes,
}

impl NgsiLdEntity {
    /// Builds an entity with the given id and type and no attributes.
    #[must_use]
    pub fn new(id: Urn, entity_type: NameBuf) -> NgsiLdEntity {
        NgsiLdEntity {
            context: None,
            id,
            entity_type,
            scope: None,
            attributes: Attributes::default(),
        }
    }
}

impl SerializeRepr for NgsiLdEntity {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;

        if let Some(context) = &self.context {
            map.serialize_entry("@context", context)?;
        }
        map.serialize_entry("id", &DisplayStr(&self.id))?;
        map.serialize_entry("type", &DisplayStr(&self.entity_type))?;
        if let Some(scope) = &self.scope {
            map.serialize_entry("scope", scope)?;
        }

        for (name, wrapper) in &self.attributes {
            if wrapper.is_skipped(skip_null) {
                continue;
            }
            map.serialize_entry(name.as_str(), &ReprAdapter::new(wrapper, representation, skip_null))?;
        }

        map.end()
    }
}

impl NgsiLdSerializable for NgsiLdEntity {
    fn to_json(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> NgsiLdResult<JsonValue> {
        Ok(serde_json::to_value(ReprAdapter::new(self, representation, skip_null))?)
    }

    fn to_string(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull, layout: JsonLayout) -> NgsiLdResult<String> {
        let adapter = ReprAdapter::new(self, representation, skip_null);
        let rendered = match layout {
            JsonLayout::Compact => serde_json::to_string(&adapter)?,
            JsonLayout::Pretty => serde_json::to_string_pretty(&adapter)?,
        };
        Ok(rendered)
    }
}

impl NgsiLdSerializable for [NgsiLdEntity] {
    fn to_json(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> NgsiLdResult<JsonValue> {
        let vals = self
            .iter()
            .map(|entity| entity.to_json(representation, skip_null))
            .collect::<NgsiLdResult<Vec<_>>>()?;
        Ok(JsonValue::Array(vals))
    }

    fn to_string(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull, layout: JsonLayout) -> NgsiLdResult<String> {
        let adapters: Vec<ReprAdapter<'_, NgsiLdEntity>> = self.iter().map(|entity| ReprAdapter::new(entity, representation, skip_null)).collect();
        let rendered = match layout {
            JsonLayout::Compact => serde_json::to_string(&adapters)?,
            JsonLayout::Pretty => serde_json::to_string_pretty(&adapters)?,
        };
        Ok(rendered)
    }
}
