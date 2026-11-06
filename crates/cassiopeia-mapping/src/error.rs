use crate::{
    conversion::{admitted_transformations::AdmittedTransformations, declaration_shape::DeclarationShape},
    template::compile_error::TemplateCompileError,
    template_location::TemplateLocation,
    transformation::Transformation,
};
use cassiopeia_common::error::io::IoError;
use cassiopeia_geometry::error::GeometryError;
use cassiopeia_ngsi_ld::{
    entity::{attribute::NgsiLdAttributeKind, attribute_path::AttributePath, error::NgsiLdError, name::NameBuf, value_type::ValueType},
    value::types::TemporalValueType,
};
use cassiopeia_value_type_expansion::error::ExpansionError;
use langtag::LangTagBuf;
use std::{path::PathBuf, result};
use thiserror::Error;

/// Failures raised while loading a mapping document.
#[derive(Debug, Error)]
pub enum MappingError {
    /// The mapping file could not be read from disk.
    #[error(transparent)]
    Io(#[from] IoError),

    /// The mapping document could not be parsed.
    #[error("Failed to parse the mapping document at '{}'", path.display())]
    Parse {
        /// The document that could not be parsed.
        path: PathBuf,
        /// The parse failure reported by the JSON5 reader.
        #[source]
        source: serde_json5::Error,
    },

    /// A template in the mapping cannot be compiled, so no record could ever render it.
    #[error("The {location} cannot be compiled")]
    UncompilableTemplate {
        /// Where the template is declared: the document and the declaration within it.
        location: TemplateLocation,
        /// The compilation failure, naming the template and what to change, and chaining Tera's own
        /// report where Tera refused it. Boxed so this variant does not enlarge every `Result`
        /// carrying the error past the large-error threshold.
        #[source]
        source: Box<TemplateCompileError>,
    },

    /// A `scope` declared as literal text can never be the scope of an entity Cassiopeia writes, so
    /// every record would be skipped for it.
    #[error("The {location} can never be an entity's scope")]
    UnwritableScope {
        /// Where the literal scope is declared: the document and the `scope` member within it.
        location: TemplateLocation,
        /// Why the literal cannot be an entity's scope.
        #[source]
        source: NgsiLdError,
    },

    /// An attribute declaration is internally inconsistent: its `geometry` conversion cannot
    /// produce the geometry type its `transformation`, or its type's default, names, so no record
    /// could ever satisfy it.
    #[error("Attribute `{attribute}` declares a geometry conversion that cannot run")]
    InvalidAttribute {
        /// The attribute whose declaration is inconsistent.
        attribute: NameBuf,
        /// Why the declared conversion cannot produce the declared type.
        #[source]
        source: GeometryError,
    },

    /// An attribute declares a `geometry` block, but its conversion reads no geometry, so the block
    /// could never apply to a single record.
    ///
    /// The conversion is the declared `transformation`, or the default of the attribute's type when
    /// none is declared: only a `GeoProperty` defaults to reading a geometry.
    #[error(
        "Attribute `{attribute}` declares a `geometry` block, but its value is not converted to a geometry; name a geometry `transformation` or remove the block"
    )]
    GeometryPolicyWithoutGeometry {
        /// The attribute whose `geometry` block could never apply.
        attribute: NameBuf,
    },

    /// A declaration names a `transformation` its type can never take from what its shape reads.
    ///
    /// The value of each NGSI-LD attribute type is defined by ETSI GS CIM 009 v1.9.1 clause 4.5: a
    /// `GeoProperty` holds a `GeoJSON` geometry (clause 4.7.1), a `JsonProperty` a JSON object or
    /// array of objects (clause 4.5.24.2), and so on, and an object built from nested `mappings` is
    /// no boolean, number, or date-time. A transformation that can never produce that value would
    /// fail or vanish on every record, so the mapping is refused naming the transformations the
    /// declaration does admit.
    #[error("Attribute `{attribute}` ({kind}, {shape}) cannot take `transformation: \"{transformation}\"`; {admitted}")]
    TransformationNotAdmitted {
        /// The declaration whose transformation cannot apply.
        attribute: NameBuf,
        /// Its NGSI-LD attribute type.
        kind: NgsiLdAttributeKind,
        /// What it is built from.
        shape: DeclarationShape,
        /// The transformation it declared.
        transformation: Transformation,
        /// The transformations it admits instead.
        admitted: AdmittedTransformations,
    },

    /// A declaration's type can never be built from what its shape reads, whatever its
    /// transformation.
    ///
    /// No object is an IRI, so a `VocabProperty` cannot be assembled from nested `mappings`; a
    /// relationship's object is built from its `source` and `target`, not from nested `mappings`;
    /// and only a `LanguageProperty` reads a `languageMap` (clause 4.5.18), so any other type
    /// declaring one would ignore it.
    #[error("Attribute `{attribute}` is a {kind}, which cannot be {shape}")]
    ShapeNotAdmitted {
        /// The declaration whose shape its type cannot take.
        attribute: NameBuf,
        /// Its NGSI-LD attribute type.
        kind: NgsiLdAttributeKind,
        /// What it is built from.
        shape: DeclarationShape,
    },

    /// A sub-attribute, or a member of a structured value, declares `instances`.
    ///
    /// Several instances of one attribute exist only on an Entity (ETSI GS CIM 009 v1.9.1 clauses
    /// 4.5.1 and 4.5.5): a sub-attribute is one serialized Property or Relationship (clauses 4.5.2.2
    /// and 4.5.3.2), and a member is one JSON member of the value holding it, so the instances would
    /// be folded into one value or lost.
    #[error(
        "Attribute `{attribute}` declares `instances`, which only a top-level attribute can: several instances of one attribute exist only on an entity (ETSI GS CIM 009 v1.9.1 clause 4.5.5), so declare one `source` instead"
    )]
    InstancesBelowTopLevel {
        /// Where the declaration sits, from its top-level attribute down.
        attribute: AttributePath,
    },

    /// A `languageMap` entry declares `instances`.
    ///
    /// Each entry of a `languageMap` is the one localized string for its language (ETSI GS CIM 009
    /// v1.9.1 clause 4.5.18), so it has nowhere to put a second instance.
    #[error(
        "Attribute `{attribute}` declares `instances` in its `languageMap` entry `{language}`, which holds one localized string (ETSI GS CIM 009 v1.9.1 clause 4.5.18); only a top-level attribute can declare instances"
    )]
    InstancesInLanguageEntry {
        /// Where the `LanguageProperty` holding the entry sits.
        attribute: AttributePath,
        /// The language the entry is keyed by.
        language: LangTagBuf,
    },

    /// A Relationship or `ListRelationship`, or one of its instances, declares a `valueType`.
    ///
    /// Neither kind has a `valueType` member (ETSI GS CIM 009 v1.9.1 tables 5.2.6-1 and 5.2.33-1):
    /// the type of what a relationship points at is its `objectType`, which Cassiopeia writes from
    /// `target.entity`.
    #[error(
        "Attribute `{attribute}` is a `{kind}`, which has no `valueType`; the type of its target is `objectType`, written from `target.entity`, so remove the `valueType`"
    )]
    ValueTypeOnRelationship {
        /// Where the relationship that declares a `valueType` sits, from its top-level attribute
        /// down.
        attribute: AttributePath,
        /// Its relationship kind.
        kind: NgsiLdAttributeKind,
    },

    /// An attribute's `properties` declare a sub-attribute named `valueType`.
    ///
    /// `valueType` is a member of the attribute itself (ETSI GS CIM 009 v1.9.1 clause 4.5.2), so a
    /// sub-attribute of that name would be written as a nested Property, which a broker refuses.
    #[error("Attribute `{attribute}` declares `valueType` under `properties`, where it would become a sub-attribute; move it up a level, next to `type`")]
    ValueTypeUnderProperties {
        /// Where the attribute whose `properties` declare it sits, from its top-level attribute down.
        attribute: AttributePath,
    },

    /// A member of an attribute's structured value, an entry of its `mappings` or `languageMap`,
    /// declares a `valueType`.
    ///
    /// Those entries build parts of one value, not attributes, so there is no member to write a
    /// `valueType` in.
    #[error(
        "Attribute `{attribute}` declares a `valueType` on an entry of its `mappings` or `languageMap`, which builds part of its value and has no `valueType`; declare it on the attribute"
    )]
    ValueTypeOnValueMember {
        /// Where the attribute the entry belongs to sits, from its top-level attribute down.
        attribute: AttributePath,
    },

    /// A declared `valueType` cannot be expanded to a datatype IRI under the `@context` its entity is
    /// written with.
    #[error("Attribute `{attribute}` declares `valueType: \"{value_type}\"`, which cannot be checked")]
    UnexpandableValueType {
        /// Where the attribute that declares it sits, from its top-level attribute down.
        attribute: AttributePath,
        /// The value type as declared, boxed so this variant, which also carries a path, does not
        /// enlarge every `Result` carrying the error past the large-error threshold.
        value_type: Box<ValueType>,
        /// Why it could not be expanded.
        #[source]
        source: Box<ExpansionError>,
    },

    /// A declared `valueType` names another datatype than the time value its `transformation`
    /// produces.
    ///
    /// A `valueType` types a Property's `value` and each item of a `ListProperty`'s `valueList`
    /// (ETSI GS CIM 009 v1.9.1 clause 4.5.2, table 5.2.36-1), and a time value already has its data
    /// type, which a Property writes on its own (Annex C.6).
    #[error(
        "Attribute `{attribute}` declares `valueType: \"{value_type}\"`, but `transformation: \"{transformation}\"` produces a `{implied}`; declare `{implied}` or remove the `valueType`"
    )]
    ValueTypeContradictsTransformation {
        /// Where the attribute whose declarations disagree sits, from its top-level attribute down.
        attribute: AttributePath,
        /// The value type as declared, boxed so this variant, which also carries a path, does not
        /// enlarge every `Result` carrying the error past the large-error threshold.
        value_type: Box<ValueType>,
        /// The transformation it declared.
        transformation: Transformation,
        /// The data type the transformation produces.
        implied: TemporalValueType,
    },

    /// A declared `valueType` names a time value data type, but the attribute's value is not read as
    /// one.
    ///
    /// A `DateTime`, `Date` or `Time` value has to be in the form ETSI GS CIM 009 v1.9.1 clause 4.6.3
    /// gives it, which only the matching transformation guarantees.
    #[error(
        "Attribute `{attribute}` declares `valueType: \"{value_type}\"`, which names a `{named}`, but its value is not read as one; add `transformation: \"{needed}\"`"
    )]
    TimeValueTypeWithoutTransformation {
        /// Where the attribute whose value is not read as a time sits, from its top-level attribute
        /// down.
        attribute: AttributePath,
        /// The value type as declared, boxed so this variant, which also carries a path, does not
        /// enlarge every `Result` carrying the error past the large-error threshold.
        value_type: Box<ValueType>,
        /// The time value data type it names.
        named: TemporalValueType,
        /// The transformation that reads a value as that data type.
        needed: Transformation,
    },

    /// A `LanguageProperty` declares a `valueType` other than `langString`.
    #[error(
        "Attribute `{attribute}` is a `LanguageProperty`, whose `valueType` can only be `langString` (ETSI GS CIM 009 v1.9.1 table 5.2.32-1), not `{value_type}`"
    )]
    LanguagePropertyValueType {
        /// Where the `LanguageProperty` that declares it sits, from its top-level attribute down.
        attribute: AttributePath,
        /// The value type as declared, boxed so this variant, which also carries a path, does not
        /// enlarge every `Result` carrying the error past the large-error threshold.
        value_type: Box<ValueType>,
    },
}

/// The result type used throughout mapping loading.
pub type Result<T, E = MappingError> = result::Result<T, E>;

#[cfg(test)]
mod tests {
    use crate::{
        error::MappingError,
        template::{TemplateSource, compile_error::TemplateCompileError, engine_report::EngineReport, identifier_lint::ambiguous_hyphen},
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use cassiopeia_common::error::io::{IoAction, IoError};
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, error::NgsiLdError, name::NameBuf};
    use pretty_assertions::assert_eq;
    use std::{
        error::Error,
        io,
        path::{Path, PathBuf},
        sync::Arc,
    };

    fn temperature() -> TemplateLocation {
        TemplateLocation::new(
            Arc::from(Path::new("/maps/sensor.json5")),
            TemplateSite::Attribute(AttributePath::top_level(NameBuf::new("temperature").unwrap())),
        )
    }

    #[test]
    fn an_uncompilable_template_names_the_document_and_the_site_and_chains_the_template() {
        let error = MappingError::UncompilableTemplate {
            location: temperature(),
            source: Box::new(TemplateCompileError::Syntax {
                template: TemplateSource::new("{{ t | upper "),
                source: EngineReport::unlocated(tera::Error::message("Unexpected end of input")),
            }),
        };

        assert_eq!(
            error.to_string(),
            "The attribute `temperature` template in the mapping document at '/maps/sensor.json5' cannot be compiled"
        );
        let template = error.source().expect("the template failure is chained");
        assert!(template.to_string().contains("{{ t | upper "));
        assert_eq!(
            template.source().expect("the engine report is chained").to_string(),
            "Tera: Unexpected end of input"
        );
    }

    #[test]
    fn an_ambiguous_hyphen_is_reported_under_the_template_headline_with_the_hint_as_its_cause() {
        let error = MappingError::UncompilableTemplate {
            location: temperature(),
            source: Box::new(TemplateCompileError::AmbiguousHyphen {
                template: TemplateSource::new("{{ station-id }}"),
                reference: ambiguous_hyphen("{{ station-id }}").unwrap(),
            }),
        };

        assert_eq!(
            error.source().expect("the hint is chained").to_string(),
            "`station-id` in `{{ station-id }}` is ambiguous: write `this['station-id']` to read the field, or `station - id` to subtract"
        );
    }

    #[test]
    fn an_unwritable_scope_names_the_document_and_chains_why_the_literal_is_no_scope() {
        let error = MappingError::UnwritableScope {
            location: TemplateLocation::new(Arc::from(Path::new("/maps/thing.json5")), TemplateSite::Scope),
            source: NgsiLdError::NullScope,
        };

        assert_eq!(
            error.to_string(),
            "The scope template in the mapping document at '/maps/thing.json5' can never be an entity's scope"
        );
        assert_eq!(error.source().expect("the reason is chained").to_string(), NgsiLdError::NullScope.to_string());
    }

    #[test]
    fn an_io_failure_is_wrapped_transparently() {
        let io = IoError::FileOperation {
            source: io::Error::other("boom"),
            path: PathBuf::from("/maps/sensor.json5"),
            action: IoAction::Read,
        };

        assert!(MappingError::from(io).to_string().contains("sensor.json5"));
    }
}
