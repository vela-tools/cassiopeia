use crate::{
    attribute::{Attribute, Attributes},
    error::{MappingError, Result},
    geometry_validation::validate,
    identity::Identity,
    observed_at::ObservedAt,
    template::{CompiledTemplate, TemplateSource, resolver::TemplateResolver, runner::TemplateRunner},
    version::Version,
};
use cassiopeia_common::error::io::{IoAction, IoError};
use cassiopeia_ngsi_ld::data_model::DataModel;
use getset::{Getters, MutGetters};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{fs::read_to_string, path::Path};

/// A mapping document: the declaration of how one source record becomes one NGSI-LD entity.
#[derive(Debug, Clone, Serialize, Deserialize, Getters, MutGetters)]
pub struct Mapping {
    /// The document format version.
    #[getset(get = "pub")]
    version: Version,

    /// The Smart Data Model this mapping targets.
    #[serde(rename = "dataModel")]
    #[getset(get = "pub")]
    data_model: DataModel,

    /// How the entity's identity is derived from a record.
    ///
    /// Mutable access exists so the expansion stage can compile the identity's templates in place
    /// after the document is loaded.
    #[getset(get = "pub", get_mut = "pub")]
    identity: Identity,

    /// The attribute declarations.
    ///
    /// Mutable access exists so the expansion stage can compile each attribute's templates in place
    /// after the document is loaded.
    #[getset(get = "pub", get_mut = "pub")]
    attributes: Attributes,

    /// The `observedAt` template found anywhere in the attribute tree and compiled at load time,
    /// so temporality is an O(1) check rather than a repeated traversal.
    #[serde(skip)]
    #[getset(get = "pub")]
    temporal_source: Option<TemplateSource>,

    /// The compiled form of `temporal_source`.
    #[serde(skip)]
    #[getset(get = "pub")]
    observed_at_template: Option<CompiledTemplate>,

    /// Whether any attribute declares a nested Relationship or `ListRelationship` sub-attribute,
    /// computed once at load time so the expander can gate its nested-minting recursion on an O(1)
    /// check (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with 4.5.3).
    #[serde(skip)]
    has_nested_relationships: bool,
}

impl Mapping {
    /// Declares a mapping and resolves its temporal fields.
    pub fn new(version: Version, data_model: DataModel, identity: Identity, attributes: Attributes, runner: &mut TemplateRunner) -> Mapping {
        let mut mapping = Mapping {
            version,
            data_model,
            identity,
            attributes,
            temporal_source: None,
            observed_at_template: None,
            has_nested_relationships: false,
        };
        mapping.compile_temporal_fields(runner);
        mapping.has_nested_relationships = mapping.detect_nested_relationships();

        mapping
    }

    /// Reads a mapping document from disk.
    ///
    /// # Errors
    /// Returns a [`MappingError`] when the file cannot be read, parsed, or its templates compiled.
    pub fn from_file(path: &Path, runner: &mut TemplateRunner) -> Result<Mapping> {
        let content = read_to_string(path).map_err(|source| IoError::FileOperation {
            source,
            path: path.to_path_buf(),
            action: IoAction::Read,
        })?;

        Mapping::from_json5(&content, path, runner)
    }

    /// Parses a mapping document held in memory. `origin` names the document in error messages.
    ///
    /// # Errors
    /// Returns a [`MappingError`] when the document cannot be parsed or its templates compiled.
    pub fn from_json5(content: &str, origin: &Path, runner: &mut TemplateRunner) -> Result<Mapping> {
        let mut mapping: Mapping = serde_json5::from_str(content).map_err(|source| MappingError::Parse {
            path: origin.to_path_buf(),
            source,
        })?;
        validate(&mapping)?;
        mapping.compile_temporal_fields(runner);
        mapping.has_nested_relationships = mapping.detect_nested_relationships();

        Ok(mapping)
    }

    /// Every attribute declared anywhere in this mapping, nested ones included.
    pub fn iter_all_attributes(&self) -> impl Iterator<Item = &Attribute> {
        self.attributes.values().flat_map(Attribute::iter_recursive)
    }

    /// Whether this mapping produces temporal entities, that is, whether any attribute carries an
    /// `observedAt` declaration.
    #[must_use]
    pub const fn is_temporal(&self) -> bool {
        self.temporal_source.is_some()
    }

    /// Whether any attribute declares a nested Relationship or `ListRelationship` sub-attribute.
    ///
    /// The expander gates its nested-relationship minting recursion on this, so a mapping without any
    /// nested relationship pays a single boolean check and follows the top-level path unchanged (ETSI
    /// GS CIM 009 v1.9.1 clause 4.5.2.2 with 4.5.3).
    #[must_use]
    pub const fn has_nested_relationships(&self) -> bool {
        self.has_nested_relationships
    }

    /// Reads the `observedAt` value for one record.
    ///
    /// Returns `None` when the mapping is not temporal, or when the record's timestamp field is
    /// absent or empty; a missing timestamp drops the temporal qualifier rather than the record.
    #[must_use]
    pub fn extract_observed_at(&self, resolver: &TemplateResolver, data: &JsonValue) -> Option<ObservedAt> {
        let template = self.observed_at_template.as_ref()?;

        match resolver.resolve(template, data).ok()? {
            JsonValue::String(value) if !value.is_empty() => Some(ObservedAt::new(value)),
            JsonValue::Number(value) => Some(ObservedAt::new(value.to_string())),
            JsonValue::String(_) | JsonValue::Null | JsonValue::Bool(_) | JsonValue::Array(_) | JsonValue::Object(_) => None,
        }
    }

    /// Whether any top-level attribute declares a nested relationship in its `properties` subtree.
    fn detect_nested_relationships(&self) -> bool {
        self.attributes.values().any(Attribute::declares_nested_relationship)
    }

    /// Finds and compiles the `observedAt` template, once, at load time.
    fn compile_temporal_fields(&mut self, runner: &mut TemplateRunner) {
        let temporal_source = self.iter_all_attributes().find_map(Attribute::observed_at_source);

        self.observed_at_template = temporal_source.as_ref().map(|source| runner.compile(source));
        self.temporal_source = temporal_source;
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::MappingError, mapping::Mapping, observed_at::ObservedAt, template::runner::TemplateRunner, version::Version};
    use serde_json::json;
    use std::path::Path;

    const TEMPORAL: &str = r#"{
        version: "v4",
        dataModel: "dataModel.Environment/AirQualityObserved",
        identity: { entityName: "Station-{{ id }}", scope: "/test" },
        attributes: {
            temperature: {
                source: "{{ temperature }}",
                transformation: "float",
                properties: { observedAt: { source: "{{ timestamp }}" } },
            },
        },
    }"#;

    const NON_TEMPORAL: &str = r#"{
        version: "v4",
        dataModel: "AirQualityObserved",
        identity: { entityName: "Station-{{ id }}" },
        attributes: { temperature: { source: "{{ temperature }}" } },
    }"#;

    fn parse(document: &str) -> Mapping {
        Mapping::from_json5(document, Path::new("test.json5"), &mut TemplateRunner::new()).unwrap()
    }

    #[test]
    fn reads_the_version_and_data_model() {
        let mapping = parse(TEMPORAL);

        assert_eq!(mapping.version(), &Version::V4);
        assert_eq!(mapping.data_model().entity_type().as_str(), "AirQualityObserved");
    }

    #[test]
    fn a_mapping_declaring_observed_at_is_temporal() {
        assert!(parse(TEMPORAL).is_temporal());
    }

    #[test]
    fn a_mapping_without_observed_at_is_not_temporal() {
        let mapping = parse(NON_TEMPORAL);

        assert!(!mapping.is_temporal());
        assert_eq!(mapping.temporal_source(), &None);
    }

    #[test]
    fn extracts_the_observed_at_value_from_a_record() {
        let mut runner = TemplateRunner::new();
        let mapping = Mapping::from_json5(TEMPORAL, Path::new("test.json5"), &mut runner).unwrap();
        let record = json!({"id": 1, "temperature": 21.5, "timestamp": "2026-04-03T22:00:20Z"});

        assert_eq!(
            mapping.extract_observed_at(&runner.resolver(), &record),
            Some(ObservedAt::new("2026-04-03T22:00:20Z"))
        );
    }

    #[test]
    fn a_numeric_observed_at_value_is_stringified() {
        let mut runner = TemplateRunner::new();
        let mapping = Mapping::from_json5(TEMPORAL, Path::new("test.json5"), &mut runner).unwrap();
        let record = json!({"id": 1, "timestamp": 1_775_253_620});

        assert_eq!(mapping.extract_observed_at(&runner.resolver(), &record), Some(ObservedAt::new("1775253620")));
    }

    #[test]
    fn a_missing_observed_at_field_yields_no_timestamp() {
        let mut runner = TemplateRunner::new();
        let mapping = Mapping::from_json5(TEMPORAL, Path::new("test.json5"), &mut runner).unwrap();

        assert_eq!(mapping.extract_observed_at(&runner.resolver(), &json!({"id": 1})), None);
    }

    #[test]
    fn a_non_temporal_mapping_never_yields_a_timestamp() {
        let mut runner = TemplateRunner::new();
        let mapping = Mapping::from_json5(NON_TEMPORAL, Path::new("test.json5"), &mut runner).unwrap();
        let record = json!({"timestamp": "2026-04-03T22:00:20Z"});

        assert_eq!(mapping.extract_observed_at(&runner.resolver(), &record), None);
    }

    #[test]
    fn walks_every_attribute_including_nested_ones() {
        let document = r#"{
            version: "v4",
            dataModel: "Sensor",
            identity: { entityName: "S-{{ id }}" },
            attributes: {
                address: { mappings: { city: { source: "{{ city }}" }, street: { source: "{{ street }}" } } },
            },
        }"#;

        assert_eq!(parse(document).iter_all_attributes().count(), 3);
    }

    #[test]
    fn a_mapping_with_no_nested_relationship_reports_false() {
        assert!(!parse(NON_TEMPORAL).has_nested_relationships());
    }

    #[test]
    fn a_relationship_carrying_a_nested_relationship_is_detected() {
        let document = r#"{
            version: "v4",
            dataModel: "Movie",
            identity: { entityName: "M-{{ id }}" },
            attributes: {
                hasLeadActor: {
                    type: "Relationship",
                    target: { entity: "Person" },
                    source: "{{ actor }}",
                    properties: {
                        playsCharacter: { type: "Relationship", target: { entity: "Character" }, source: "{{ character }}" },
                    },
                },
            },
        }"#;

        assert!(parse(document).has_nested_relationships());
    }

    #[test]
    fn a_property_carrying_only_qualifiers_is_not_a_nested_relationship() {
        let document = r#"{
            version: "v4",
            dataModel: "Sensor",
            identity: { entityName: "S-{{ id }}" },
            attributes: {
                temperature: {
                    source: "{{ t }}",
                    properties: { unitCode: { source: "CEL" }, observedAt: { source: "{{ ts }}" } },
                },
            },
        }"#;

        assert!(!parse(document).has_nested_relationships());
    }

    #[test]
    fn a_missing_version_is_rejected() {
        let document = r#"{
            dataModel: "Sensor",
            identity: { entityName: "S-{{ id }}" },
            attributes: {},
        }"#;

        assert!(Mapping::from_json5(document, Path::new("test.json5"), &mut TemplateRunner::new()).is_err());
    }

    #[test]
    fn a_superseded_version_is_rejected() {
        let document = r#"{
            version: "v3",
            dataModel: "Sensor",
            identity: { entityName: "S-{{ id }}" },
            attributes: {},
        }"#;

        assert!(Mapping::from_json5(document, Path::new("test.json5"), &mut TemplateRunner::new()).is_err());
    }

    #[test]
    fn a_superseded_urn_identity_is_rejected() {
        let document = r#"{
            version: "v4",
            dataModel: "Sensor",
            identity: { entityName: "S-{{ id }}", urn: "something" },
            attributes: {},
        }"#;

        assert!(Mapping::from_json5(document, Path::new("test.json5"), &mut TemplateRunner::new()).is_err());
    }

    #[test]
    fn reading_a_document_that_does_not_exist_is_a_read_error() {
        let result = Mapping::from_file(Path::new("does-not-exist.json5"), &mut TemplateRunner::new());

        assert!(matches!(result, Err(MappingError::Io(_))));
    }
}
