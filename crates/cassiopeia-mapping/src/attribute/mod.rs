pub mod instance;
pub mod iterator;

use crate::{
    attribute::{instance::AttributeInstance, iterator::AttributeIterator},
    mapping::Mapping,
    target::Target,
    template::{CompiledTemplate, TemplateSource},
    transformation::Transformation,
};
use cassiopeia_geometry::policy::GeometryPolicy;
use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};
use getset::{Getters, MutGetters, Setters};
use indexmap::IndexMap;
use langtag::LangTagBuf;
use serde::{Deserialize, Deserializer, Serialize, de::Error as DeserializeError};
use serde_json::Value as JsonValue;
use typed_builder::TypedBuilder;

/// A mapping's attribute declarations, keyed by the NGSI-LD attribute name they produce.
pub type Attributes = IndexMap<NameBuf, Attribute>;

/// The per-language declarations of a `LanguageProperty`, keyed by BCP-47 language tag.
///
/// A `languageMap` is defined over language tags, not attribute names (ETSI GS CIM 009 v1.9.1
/// clause 4.5.18), so its keys validate as BCP-47 tags: `pt-BR` and `zh-CN` are legal keys here
/// even though they are not legal NGSI-LD attribute names.
pub type LanguageMap = IndexMap<LangTagBuf, Attribute>;

/// The attribute type produced when a mapping omits `type`.
///
/// NGSI-LD's own default for an unqualified attribute is a Property (ETSI GS CIM 009 v1.9.1
/// clause 4.5.2), so a mapping that names a source field and nothing else produces one.
const fn default_kind() -> NgsiLdAttributeKind {
    NgsiLdAttributeKind::Property
}

/// One attribute declaration: either a leaf that reads source values, or a node holding nested
/// attribute declarations, or both.
#[derive(Debug, Clone, Serialize, Deserialize, Getters, MutGetters, Setters, TypedBuilder)]
pub struct Attribute {
    /// Which NGSI-LD attribute type to build.
    #[serde(rename = "type", default = "default_kind")]
    #[builder(default = default_kind())]
    #[getset(get = "pub")]
    kind: NgsiLdAttributeKind,

    /// The conversion applied to the extracted value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    #[getset(get = "pub")]
    transformation: Option<Transformation>,

    /// What may be lost bringing this attribute's geometry to the type `transformation` names.
    ///
    /// Absent means lossless-only: identity, promotion to a multi-geometry, and unwrapping a
    /// multi-geometry of exactly one member all pass, and any conversion that would discard
    /// coordinates is refused. An attribute's instances share this policy, exactly as they share
    /// its `type` and `transformation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    #[getset(get = "pub")]
    geometry: Option<GeometryPolicy>,

    /// The template, or list of templates, producing this attribute's value. Absent on nodes that
    /// only carry nested declarations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    #[getset(get = "pub")]
    source: Option<JsonValue>,

    /// Nested attribute declarations, for an attribute whose value is a structured object.
    ///
    /// Mutable access exists so the expansion stage can compile nested templates in place.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[builder(default = Default::default())]
    #[getset(get = "pub", get_mut = "pub")]
    mappings: Attributes,

    /// Per-language declarations backing a `LanguageProperty`'s `languageMap`, keyed by BCP-47
    /// language tag.
    ///
    /// Mutable access exists so the expansion stage can compile per-language templates in place.
    #[serde(default, rename = "languageMap", skip_serializing_if = "IndexMap::is_empty")]
    #[builder(default = Default::default())]
    #[getset(get = "pub", get_mut = "pub")]
    language_map: LanguageMap,

    /// A whole further entity derived from the same record, emitted alongside this one.
    ///
    /// A synthetic entity is an entity body without its own document `version`; it shares the
    /// parent document's version and is lifted into a full [`Mapping`] at load time (see
    /// [`deserialize_synthetic_entity`]). Mutable access exists so the expansion stage can compile
    /// the synthetic mapping in place.
    #[serde(
        default,
        rename = "syntheticEntity",
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_synthetic_entity"
    )]
    #[builder(default = Default::default())]
    #[getset(get = "pub", get_mut = "pub")]
    synthetic_entity: Option<Mapping>,

    /// The entity a relationship attribute points at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    #[getset(get = "pub")]
    target: Option<Target>,

    /// Attribute-level sub-attributes: qualifiers such as `observedAt` or `unitCode`, and any further
    /// nested attribute the spec admits.
    ///
    /// A sub-attribute is the serialization of a Property or any of its subclasses, and of a
    /// Relationship (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with 4.5.3), so this map may declare a
    /// nested Relationship or `ListRelationship` as readily as a nested Property, recursively, to any
    /// depth. Mutable access exists so the expansion stage can compile sub-attribute templates in
    /// place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    #[getset(get = "pub", get_mut = "pub")]
    properties: Option<Attributes>,

    /// Several instances of this attribute, one per `datasetId` (ETSI GS CIM 009 v1.9.1 clause
    /// 4.5.5).
    ///
    /// When present, this attribute produces one instance per entry rather than a single value: each
    /// instance reads its own `source` and `datasetId`, sharing this attribute's `type`,
    /// `transformation`, and `properties`. Multi-attribute instances are valid on every reified
    /// attribute kind clause 4.5.5 permits (Property and its subtypes, `Relationship`, and
    /// `ListRelationship`), differing only in what each instance's `source` denotes: a value for a
    /// Property, an object-id for a Relationship, an object-id list for a `ListRelationship`. Mutable
    /// access exists so the expansion stage can compile per-instance templates in place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    #[getset(get = "pub", get_mut = "pub")]
    instances: Option<Vec<AttributeInstance>>,

    /// The compiled `source` templates, filled in once the mapping is loaded.
    #[serde(skip)]
    #[builder(default = Default::default())]
    #[getset(get = "pub", set = "pub")]
    compiled_source: Option<Vec<CompiledTemplate>>,
}

/// Deserializes a `syntheticEntity` declaration into a [`Mapping`].
///
/// A synthetic entity is an entity body without its own document `version`: it is emitted from the
/// same record as its parent and shares the parent document's version. Only one mapping version
/// exists (v4), so a nested body that omits `version` is lifted into a full mapping by supplying
/// that sole version; a body that names a version keeps it, and an unsupported one is still
/// rejected by [`Version`](crate::version::Version). The top-level document remains strictly
/// required to declare its own `version`, because it is parsed by the derived deserializer, not
/// this function.
fn deserialize_synthetic_entity<'de, D>(deserializer: D) -> Result<Option<Mapping>, D::Error>
where
    D: Deserializer<'de>,
{
    // `deserialize_with` runs only when the field is present, so the value is never absent here; an
    // omitted `syntheticEntity` takes the field's `default` (None) without reaching this function.
    let mut value = JsonValue::deserialize(deserializer)?;
    if let Some(object) = value.as_object_mut() {
        object.entry("version").or_insert_with(|| JsonValue::String("v4".to_string()));
    }
    let mapping = serde_json::from_value(value).map_err(DeserializeError::custom)?;

    Ok(Some(mapping))
}

impl Attribute {
    /// Walks this attribute and everything nested beneath it, depth-first.
    #[must_use]
    pub fn iter_recursive(&self) -> AttributeIterator<'_> {
        AttributeIterator::new(self)
    }

    /// The raw `observedAt` template declared directly on this attribute, if any.
    ///
    /// A non-string declaration is stringified rather than rejected: `observedAt` may be written
    /// as a number when the source carries an epoch timestamp.
    #[must_use]
    pub fn observed_at_source(&self) -> Option<TemplateSource> {
        let observed_at = NameBuf::new("observedAt").ok()?;

        match self.properties.as_ref()?.get(&observed_at)?.source.as_ref()? {
            JsonValue::String(source) => Some(TemplateSource::new(source)),
            other @ (JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_)) => {
                Some(TemplateSource::new(other.to_string()))
            }
        }
    }

    /// The first `observedAt` template found on this attribute or anything nested beneath it.
    #[must_use]
    pub fn find_observed_at_source(&self) -> Option<TemplateSource> {
        self.iter_recursive().find_map(Attribute::observed_at_source)
    }

    /// Whether this attribute's `properties` subtree declares a nested Relationship or
    /// `ListRelationship` anywhere within it.
    ///
    /// A relationship carried as a sub-attribute is a nested relationship (ETSI GS CIM 009 v1.9.1
    /// clause 4.5.2.2 with 4.5.3), whose object the expander mints under a multi-segment path. Only
    /// the `properties` subtree is walked, since a nested relationship is a sub-attribute; nested
    /// object `mappings` and language maps carry structured values, not sub-attributes.
    #[must_use]
    pub fn declares_nested_relationship(&self) -> bool {
        self.properties.iter().flat_map(IndexMap::values).any(|property| {
            matches!(property.kind(), NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship) || property.declares_nested_relationship()
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute::Attribute, template::TemplateSource, transformation::Transformation, version::Version};
    use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
    use langtag::LangTagBuf;

    fn parse(document: &str) -> Attribute {
        serde_json5::from_str(document).unwrap()
    }

    #[test]
    fn an_attribute_without_a_type_becomes_a_property() {
        let attribute = parse(r#"{"source": "{{ temperature }}"}"#);

        assert_eq!(attribute.kind(), &NgsiLdAttributeKind::Property);
    }

    #[test]
    fn the_attribute_type_is_read_from_its_ngsi_ld_token() {
        let attribute = parse(r#"{"type": "GeoProperty", "transformation": "point"}"#);

        assert_eq!(attribute.kind(), &NgsiLdAttributeKind::GeoProperty);
        assert_eq!(attribute.transformation(), &Some(Transformation::Point));
    }

    #[test]
    fn nested_mappings_are_read() {
        let attribute = parse(r#"{"mappings": {"city": {"source": "{{ city }}"}}}"#);

        assert_eq!(attribute.mappings().len(), 1);
    }

    #[test]
    fn observed_at_is_read_from_the_attribute_properties() {
        let attribute = parse(r#"{"source": "{{ t }}", "properties": {"observedAt": {"source": "{{ timestamp }}"}}}"#);

        assert_eq!(attribute.observed_at_source(), Some(TemplateSource::new("{{ timestamp }}")));
    }

    #[test]
    fn an_attribute_without_properties_has_no_observed_at() {
        assert_eq!(parse(r#"{"source": "{{ t }}"}"#).observed_at_source(), None);
    }

    #[test]
    fn observed_at_is_found_through_a_nested_mapping() {
        let attribute = parse(r#"{"mappings": {"inner": {"source": "{{ t }}", "properties": {"observedAt": {"source": "{{ timestamp }}"}}}}}"#);

        assert_eq!(attribute.observed_at_source(), None);
        assert_eq!(attribute.find_observed_at_source(), Some(TemplateSource::new("{{ timestamp }}")));
    }

    #[test]
    fn the_recursive_walk_visits_nested_mappings_language_maps_and_properties() {
        let attribute = parse(
            r#"{
                "mappings": {"a": {"source": "{{ a }}"}},
                "languageMap": {"en": {"source": "{{ en }}"}},
                "properties": {"observedAt": {"source": "{{ t }}"}}
            }"#,
        );

        assert_eq!(attribute.iter_recursive().count(), 4);
    }

    #[test]
    fn an_attribute_name_that_is_not_a_valid_ngsi_ld_name_is_rejected() {
        assert!(serde_json5::from_str::<Attribute>(r#"{"mappings": {"9bad": {"source": "{{ a }}"}}}"#).is_err());
    }

    #[test]
    fn a_language_map_accepts_region_qualified_tags_that_are_not_valid_attribute_names() {
        // `pt-BR` and `zh-CN` are legal BCP-47 tags but illegal NGSI-LD attribute names; a
        // languageMap is keyed by tag, so both must parse (ETSI GS CIM 009 v1.9.1 clause 4.5.18).
        let attribute = parse(r#"{"type": "LanguageProperty", "languageMap": {"pt-BR": {"source": "{{ pt }}"}, "zh-CN": {"source": "{{ zh }}"}}}"#);

        let tags: Vec<&str> = attribute.language_map().keys().map(LangTagBuf::as_str).collect();
        assert_eq!(tags, ["pt-BR", "zh-CN"]);
    }

    #[test]
    fn a_language_map_key_that_is_not_a_valid_language_tag_is_rejected() {
        assert!(serde_json5::from_str::<Attribute>(r#"{"type": "LanguageProperty", "languageMap": {"123": {"source": "{{ a }}"}}}"#).is_err());
    }

    #[test]
    fn a_synthetic_entity_without_a_version_inherits_the_document_version() {
        let attribute = parse(
            r#"{
                "type": "Property",
                "syntheticEntity": {
                    "dataModel": "dataModel.Hl7/Organization",
                    "identity": {"entityName": "Owner"},
                    "attributes": {"name": {"source": "Owner"}}
                }
            }"#,
        );

        let synthetic = attribute.synthetic_entity().as_ref().expect("synthetic entity is present");
        assert_eq!(synthetic.version(), &Version::V4);
        assert_eq!(synthetic.data_model().entity_type().as_str(), "Organization");
    }
}
