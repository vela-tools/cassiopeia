use crate::{editor::EditorAttribute, screen::wizard::editor_key::EditorKey};
use cassiopeia_mapping::{
    attribute::{Attribute, Attributes, LanguageMap},
    target::Target,
    transformation::Transformation,
};
use cassiopeia_ngsi_ld::{
    data_model::DataModel,
    entity::{attribute::NgsiLdAttributeKind, error::NgsiLdError, name::NameBuf},
};
use indexmap::IndexMap;
use langtag::{InvalidLangTag, LangTagBuf};
use std::{result, str::FromStr};
use thiserror::Error;

/// A failure raised while turning the wizard's editing tree into a mapping document.
///
/// The editing tree carries free-text names and target models the user typed; validation happens
/// here, at the boundary where those strings must become the mapping crate's validated types.
#[derive(Debug, Error)]
pub enum ConversionError {
    /// An attribute name in the editing tree is not a valid NGSI-LD attribute name.
    #[error("'{name}' is not a valid attribute name: {source}")]
    AttributeName { name: String, source: NgsiLdError },

    /// A relationship's target model is not a valid NGSI-LD model identifier.
    #[error("'{entity}' is not a valid target model: {source}")]
    TargetEntity { entity: String, source: NgsiLdError },

    /// A `languageMap` key in the editing tree is not a valid BCP-47 language tag.
    #[error("'{tag}' is not a valid BCP-47 language tag: {source}")]
    LanguageTag { tag: String, source: InvalidLangTag<String> },
}

pub type Result<T, E = ConversionError> = result::Result<T, E>;

/// Reduces the editing tree to the attributes that will actually be written.
///
/// A leaf attribute survives only once it has a source template. A `oneOf` container is collapsed to
/// the single option the user mapped: the wizard presents each `oneOf` choice as its own child, but
/// a mapping document records one concrete shape, so the first mapped option wins and its siblings
/// are dropped. Empty containers disappear entirely.
#[must_use]
pub fn filter_mapped(attributes: &IndexMap<EditorKey, EditorAttribute>) -> IndexMap<EditorKey, EditorAttribute> {
    let mut filtered = IndexMap::new();

    for (key, attribute) in attributes {
        let is_oneof_container = attribute.is_container() && attribute.mappings.keys().any(EditorKey::is_one_of_option);

        if is_oneof_container {
            if let Some((child, child_mappings)) = selected_option(attribute) {
                // `merged` is a clone of `child`, so its `language_map` already matches; only the
                // filtered nested mappings need overwriting.
                let mut merged = child.clone();
                merged.mappings = child_mappings;
                filtered.insert(key.clone(), merged);
            }
        } else if attribute.source.is_some() {
            let mut kept = attribute.clone();
            kept.mappings = if attribute.is_container() {
                filter_mapped(&attribute.mappings)
            } else {
                IndexMap::new()
            };
            filtered.insert(key.clone(), kept);
        } else if attribute.is_container() {
            let children = filter_mapped(&attribute.mappings);
            if !children.is_empty() {
                let mut kept = attribute.clone();
                kept.source = None;
                kept.mappings = children;
                filtered.insert(key.clone(), kept);
            }
        }
    }

    filtered
}

/// Picks the mapped `oneOf` option and its filtered children, if any option is mapped.
///
/// An option counts as chosen when it carries a source directly, or when filtering its children
/// leaves something behind; the first such option in declaration order wins.
fn selected_option(container: &EditorAttribute) -> Option<(&EditorAttribute, IndexMap<EditorKey, EditorAttribute>)> {
    for (child_key, child) in &container.mappings {
        if !child_key.is_one_of_option() {
            continue;
        }

        if child.source.is_some() {
            return Some((child, filter_mapped(&child.mappings)));
        }

        let child_mappings = filter_mapped(&child.mappings);
        if !child_mappings.is_empty() {
            return Some((child, child_mappings));
        }
    }

    None
}

/// Converts a filtered editing tree into mapping-crate attributes, validating every name.
///
/// # Errors
/// Returns [`ConversionError::AttributeName`] when a key is not a valid NGSI-LD attribute name, or
/// [`ConversionError::TargetEntity`] when a relationship's target model is invalid.
pub fn to_domain_attributes(attributes: &IndexMap<EditorKey, EditorAttribute>) -> Result<Attributes> {
    let mut domain = Attributes::new();

    for (key, attribute) in attributes {
        // The editing key's display text is the attribute name for a schema field; a `oneOf` choice
        // would fail name validation here, but `filter_mapped` collapses those away first.
        let name_text = key.to_string();
        let name = NameBuf::new(name_text.as_str()).map_err(|source| ConversionError::AttributeName { name: name_text, source })?;

        domain.insert(name, to_domain_attribute(attribute)?);
    }

    Ok(domain)
}

/// Converts a filtered editing tree into a mapping-crate `languageMap`, validating every key as a
/// BCP-47 language tag.
///
/// A `languageMap` is keyed by language tag rather than attribute name (ETSI GS CIM 009 v1.9.1
/// clause 4.5.18), so keys such as `pt-BR` are validated here as tags, not names.
///
/// # Errors
/// Returns [`ConversionError::LanguageTag`] when a key is not a valid BCP-47 language tag.
fn to_domain_language_map(attributes: &IndexMap<EditorKey, EditorAttribute>) -> Result<LanguageMap> {
    let mut domain = LanguageMap::new();

    for (key, attribute) in attributes {
        let tag_text = key.to_string();
        let tag = LangTagBuf::from_str(&tag_text).map_err(|source| ConversionError::LanguageTag { tag: tag_text, source })?;

        domain.insert(tag, to_domain_attribute(attribute)?);
    }

    Ok(domain)
}

/// Converts one editing-tree attribute, recursing into its nested declarations.
fn to_domain_attribute(attribute: &EditorAttribute) -> Result<Attribute> {
    let mappings = to_domain_attributes(&attribute.mappings)?;
    let language_map = to_domain_language_map(&attribute.language_map)?;
    let target = build_target(attribute)?;

    Ok(Attribute::builder()
        .kind(attribute.attribute_type)
        .transformation(attribute.transformation)
        .source(attribute.source.clone())
        .mappings(mappings)
        .language_map(language_map)
        .target(target)
        .build())
}

/// Builds a relationship target from the raw entity string, when the attribute is a relationship
/// that names one.
fn build_target(attribute: &EditorAttribute) -> Result<Option<Target>> {
    let is_relationship = matches!(
        attribute.attribute_type,
        NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship
    );

    match (is_relationship, attribute.target_entity.as_deref()) {
        (true, Some(entity)) if !entity.trim().is_empty() => {
            let model = DataModel::from_str(entity.trim()).map_err(|source| ConversionError::TargetEntity {
                entity: entity.to_string(),
                source,
            })?;

            Ok(Some(Target::builder().entity(model).build()))
        }
        (true | false, _) => Ok(None),
    }
}

/// Whether a transformation names a structured object, exposed so callers do not repeat the check.
#[must_use]
pub const fn is_object(transformation: Option<Transformation>) -> bool {
    matches!(transformation, Some(Transformation::Object))
}

#[cfg(test)]
mod tests {
    use crate::{
        editor::{
            EditorAttribute,
            conversion::{ConversionError, filter_mapped, to_domain_attributes},
        },
        screen::wizard::editor_key::EditorKey,
    };
    use cassiopeia_mapping::transformation::Transformation;
    use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};
    use indexmap::IndexMap;
    use langtag::LangTagBuf;
    use serde_json::Value;

    fn field(name: &str) -> EditorKey {
        EditorKey::SchemaField(name.to_string())
    }

    fn option(index: u32, title: &str) -> EditorKey {
        EditorKey::OneOfOption {
            index,
            title: title.to_string(),
        }
    }

    fn leaf_with_source(kind: NgsiLdAttributeKind) -> EditorAttribute {
        let mut attribute = EditorAttribute::new(kind, Some(Transformation::String));
        attribute.source = Some(Value::String("{{ field }}".to_string()));
        attribute
    }

    #[test]
    fn a_mapped_leaf_survives_filtering_and_an_unmapped_one_is_dropped() {
        let mut tree = IndexMap::new();
        tree.insert(field("mapped"), leaf_with_source(NgsiLdAttributeKind::Property));
        tree.insert(field("unmapped"), EditorAttribute::new(NgsiLdAttributeKind::Property, None));

        let filtered = filter_mapped(&tree);

        assert!(filtered.contains_key(&field("mapped")));
        assert!(!filtered.contains_key(&field("unmapped")));
    }

    #[test]
    fn a_oneof_container_collapses_to_the_mapped_option() {
        let mut container = EditorAttribute::new(NgsiLdAttributeKind::Property, Some(Transformation::Object));
        container.mappings.insert(
            option(1, "Point"),
            EditorAttribute::new(NgsiLdAttributeKind::GeoProperty, Some(Transformation::Point)),
        );
        container.mappings.insert(option(2, "Address"), leaf_with_source(NgsiLdAttributeKind::Property));

        let mut tree = IndexMap::new();
        tree.insert(field("location"), container);

        let filtered = filter_mapped(&tree);
        let collapsed = filtered.get(&field("location")).expect("the container is kept because one option is mapped");

        assert_eq!(collapsed.attribute_type, NgsiLdAttributeKind::Property);
        assert_eq!(collapsed.source, Some(Value::String("{{ field }}".to_string())));
    }

    #[test]
    fn a_valid_attribute_name_converts_to_the_domain_type() {
        let mut tree = IndexMap::new();
        tree.insert(field("temperature"), leaf_with_source(NgsiLdAttributeKind::Property));

        let domain = to_domain_attributes(&tree).unwrap();

        assert_eq!(domain.len(), 1);
        assert_eq!(domain.keys().next().unwrap().as_str(), "temperature");
    }

    #[test]
    fn an_invalid_attribute_name_is_rejected() {
        let mut tree = IndexMap::new();
        tree.insert(field("9bad"), leaf_with_source(NgsiLdAttributeKind::Property));

        assert!(matches!(to_domain_attributes(&tree), Err(ConversionError::AttributeName { .. })));
    }

    #[test]
    fn a_relationship_carries_its_validated_target_model() {
        let mut relationship = leaf_with_source(NgsiLdAttributeKind::Relationship);
        relationship.target_entity = Some("dataModel.Transportation/Road".to_string());

        let mut tree = IndexMap::new();
        tree.insert(field("refRoad"), relationship);

        let domain = to_domain_attributes(&tree).unwrap();
        let attribute = domain.get(&NameBuf::new("refRoad").unwrap()).unwrap();

        assert_eq!(attribute.target().as_ref().unwrap().entity().entity_type().as_str(), "Road");
    }

    #[test]
    fn a_relationship_with_an_invalid_target_model_is_rejected() {
        let mut relationship = leaf_with_source(NgsiLdAttributeKind::Relationship);
        relationship.target_entity = Some("9Road".to_string());

        let mut tree = IndexMap::new();
        tree.insert(field("refRoad"), relationship);

        assert!(matches!(to_domain_attributes(&tree), Err(ConversionError::TargetEntity { .. })));
    }

    #[test]
    fn a_language_property_carries_region_qualified_tags_that_are_not_valid_attribute_names() {
        let mut language_property = EditorAttribute::new(NgsiLdAttributeKind::LanguageProperty, None);
        language_property
            .language_map
            .insert(field("pt-BR"), leaf_with_source(NgsiLdAttributeKind::Property));

        let mut tree = IndexMap::new();
        tree.insert(field("name"), language_property);

        let domain = to_domain_attributes(&tree).unwrap();
        let attribute = domain.get(&NameBuf::new("name").unwrap()).unwrap();
        let tags: Vec<&str> = attribute.language_map().keys().map(LangTagBuf::as_str).collect();

        assert_eq!(tags, ["pt-BR"]);
    }

    #[test]
    fn a_language_map_key_that_is_not_a_valid_language_tag_is_rejected() {
        let mut language_property = EditorAttribute::new(NgsiLdAttributeKind::LanguageProperty, None);
        language_property
            .language_map
            .insert(field("123"), leaf_with_source(NgsiLdAttributeKind::Property));

        let mut tree = IndexMap::new();
        tree.insert(field("name"), language_property);

        assert!(matches!(to_domain_attributes(&tree), Err(ConversionError::LanguageTag { .. })));
    }
}
