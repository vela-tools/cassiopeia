use crate::{
    attribute::{resolution_context::ResolutionContext, resolver::resolve},
    error::Result,
};
use cassiopeia_ir::{
    metadata::MetadataStorage,
    relationship_path::RelationshipPath,
    sub_attribute::{SubAttribute, SubAttributes},
};
use cassiopeia_mapping::attribute::{Attribute, Attributes, instance::AttributeInstance};
use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};
use serde_json::Value as JsonValue;

/// Extracts an attribute's properties (such as `observedAt` or `unitCode`) into metadata storage.
pub(crate) struct MetadataExtractor;

impl MetadataExtractor {
    /// Extracts metadata for one attribute, choosing shared or per-instance storage by its shape.
    ///
    /// A multi-instance attribute carries one metadata entry per `datasetId`-tagged instance (ETSI GS
    /// CIM 009 v1.9.1 clause 4.5.5); every other kind, a `ListRelationship` included, shares one
    /// metadata entry across the attribute: its sub-attributes qualify the attribute as a whole
    /// (clause 4.5.2.2).
    pub(crate) fn extract(ctx: &ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<MetadataStorage>> {
        if let Some(instances) = config.instances() {
            Self::extract_instances(ctx, attr_name, config, instances)
        } else {
            Self::extract_shared(ctx, attr_name, config)
        }
    }

    /// Resolves the attribute's properties once, as metadata shared across the whole attribute.
    fn extract_shared(ctx: &ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<MetadataStorage>> {
        let base = Self::base_path(ctx, attr_name);
        let metadata = Self::resolve_property_map(ctx, &base, config.properties().as_ref())?;

        if metadata.is_empty() {
            Ok(None)
        } else {
            Ok(Some(MetadataStorage::shared(metadata)))
        }
    }

    /// The path to an attribute, used as the base for looking up its nested relationships. Empty when
    /// the entity has no nested relationships, so the common case allocates nothing.
    fn base_path(ctx: &ResolutionContext, attr_name: &NameBuf) -> Vec<NameBuf> {
        if ctx.nested_relationships.is_some() {
            vec![attr_name.clone()]
        } else {
            Vec::new()
        }
    }

    /// Resolves per-instance metadata for a multi-instance attribute.
    ///
    /// Each instance's metadata is the attribute's shared properties (such as `unitCode` or
    /// `observedAt`) overlaid with the instance's own (notably its `datasetId`), so an instance's key
    /// of the same name wins. Entry `i` of the result aligns with instance `i`'s resolved value (ETSI
    /// GS CIM 009 v1.9.1 clause 4.5.5).
    fn extract_instances(ctx: &ResolutionContext, attr_name: &NameBuf, config: &Attribute, instances: &[AttributeInstance]) -> Result<Option<MetadataStorage>> {
        let base = Self::base_path(ctx, attr_name);
        let shared = Self::resolve_property_map(ctx, &base, config.properties().as_ref())?;
        let kind = *config.kind();

        let mut per_item = Vec::with_capacity(instances.len());
        for instance in instances {
            // A relationship instance whose source resolves to no object was dropped by the expander
            // when it minted the objects, so its metadata is dropped too and the per-index metadata
            // stays in lockstep with the objects. A Property-family instance keeps its entry even when
            // absent: its value side pads the hole with a null the transformer drops (clause 4.5.5).
            if Self::relationship_instance_is_empty(ctx, kind, instance)? {
                continue;
            }
            // Each instance owns its merged metadata, so the shared base is cloned per instance.
            let mut metadata = shared.clone();
            let own = Self::resolve_property_map(ctx, &base, instance.properties().as_ref())?;
            metadata.extend(own);
            per_item.push(metadata);
        }

        if per_item.is_empty() {
            Ok(None)
        } else {
            Ok(Some(MetadataStorage::per_item(per_item)))
        }
    }

    /// Whether a relationship instance contributes no object, so it and its metadata are dropped.
    ///
    /// Mirrors the expander's drop decision exactly: a `Relationship` instance is empty when its
    /// source resolves to an empty identifier, and a `ListRelationship` instance when it tokenizes to
    /// nothing. A Property-family instance is never dropped here (it is null-padded on the value
    /// side), so it always reports non-empty.
    fn relationship_instance_is_empty(ctx: &ResolutionContext, kind: NgsiLdAttributeKind, instance: &AttributeInstance) -> Result<bool> {
        let Some(templates) = instance.compiled_source().as_ref() else {
            return Ok(matches!(kind, NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship));
        };

        match kind {
            NgsiLdAttributeKind::Relationship => Ok(ctx.resolver.resolve_joined(templates, ctx.data)?.is_empty()),
            NgsiLdAttributeKind::ListRelationship => Ok(ctx.resolver.resolve_tokens(templates, ctx.data)?.is_empty()),
            NgsiLdAttributeKind::Property
            | NgsiLdAttributeKind::GeoProperty
            | NgsiLdAttributeKind::LanguageProperty
            | NgsiLdAttributeKind::VocabProperty
            | NgsiLdAttributeKind::ListProperty
            | NgsiLdAttributeKind::JsonProperty => Ok(false),
        }
    }

    /// Resolves a set of sub-attribute declarations against the record into a metadata map, dropping
    /// any that resolve to null.
    ///
    /// `path` is the path to the attribute owning these sub-attributes, used only to key nested
    /// relationships; it is empty (and never grown) when the entity has no nested relationships, so
    /// the common metadata path allocates nothing extra.
    ///
    /// Each sub-attribute keeps the NGSI-LD kind it declared and, recursively, its own
    /// sub-attributes, so a nested Property subclass survives to arbitrary depth (ETSI GS CIM 009
    /// v1.9.1 clause 4.5.2.2). A nested Relationship or `ListRelationship` reads its pre-minted
    /// object(s) by path rather than being resolved as a value (clause 4.5.3); one absent from the
    /// entity's nested relationships is dropped, in lockstep with the expander's minting.
    fn resolve_property_map(ctx: &ResolutionContext, path: &[NameBuf], properties: Option<&Attributes>) -> Result<SubAttributes> {
        let mut metadata = SubAttributes::default();
        let Some(properties) = properties else {
            return Ok(metadata);
        };

        for (name, property) in properties {
            if ctx.nested_relationships.is_some() && matches!(property.kind(), NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship) {
                if let Some(sub) = Self::resolve_nested_relationship(ctx, path, name, property)? {
                    metadata.insert(name.clone(), sub);
                }
                continue;
            }

            let mut child = ctx.child(ctx.data);
            let value = resolve(&mut child, name, property)?;
            if !value.is_null() {
                // Grow the path only when nested relationships exist, so a deeper one under this
                // Property sub-attribute can still be keyed; otherwise recurse with an empty path.
                let nested = if ctx.nested_relationships.is_some() {
                    let mut deeper = path.to_vec();
                    deeper.push(name.clone());
                    Self::resolve_property_map(&child, &deeper, property.properties().as_ref())?
                } else {
                    Self::resolve_property_map(&child, &[], property.properties().as_ref())?
                };
                metadata.insert(name.clone(), SubAttribute::new(*property.kind(), value.into_json_value(), nested));
            }
        }

        Ok(metadata)
    }

    /// Builds a nested Relationship/`ListRelationship` sub-attribute from its pre-minted object(s).
    ///
    /// The object URNs were minted by the expander and travel on the entity keyed by the same
    /// [`RelationshipPath`] rebuilt here (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with 4.5.3). A path
    /// absent from the entity's nested relationships (an optional foreign key missing from this
    /// record) yields `None`, dropping the sub-attribute exactly as the expander minted no object for
    /// it. The relationship's own sub-attributes recurse under its full path.
    fn resolve_nested_relationship(ctx: &ResolutionContext, path: &[NameBuf], name: &NameBuf, property: &Attribute) -> Result<Option<SubAttribute>> {
        let Some(nested_map) = ctx.nested_relationships else {
            return Ok(None);
        };

        let mut segments = path.to_vec();
        segments.push(name.clone());
        let rel_path = RelationshipPath::from_segments(segments);

        let Some(objects) = nested_map.get(&rel_path).filter(|objects| !objects.is_empty()) else {
            return Ok(None);
        };

        let kind = *property.kind();
        let value = match kind {
            NgsiLdAttributeKind::Relationship => JsonValue::String(objects[0].to_string()),
            NgsiLdAttributeKind::ListRelationship => JsonValue::Array(objects.iter().map(|object| JsonValue::String(object.to_string())).collect()),
            NgsiLdAttributeKind::Property
            | NgsiLdAttributeKind::GeoProperty
            | NgsiLdAttributeKind::LanguageProperty
            | NgsiLdAttributeKind::VocabProperty
            | NgsiLdAttributeKind::ListProperty
            | NgsiLdAttributeKind::JsonProperty => return Ok(None),
        };

        let object_type = property
            .target()
            .as_ref()
            .and_then(|target| NameBuf::new(target.entity().entity_type().as_str()).ok());
        let nested = Self::resolve_property_map(&ctx.child(ctx.data), rel_path.segments(), property.properties().as_ref())?;

        Ok(Some(SubAttribute::new_relationship(kind, value, object_type, nested)))
    }
}
