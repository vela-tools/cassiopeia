use crate::{
    attribute::{Attribute, Attributes},
    conversion::{
        Conversion,
        conversion_refusal::ConversionRefusal,
        declaration_shape::{DeclarationShape, ValueOrigin},
    },
    error::{MappingError, Result},
};
use cassiopeia_geometry::lattice::check;
use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};

/// Resolves the conversion of every declaration in `attributes` and of everything nested beneath
/// them, stores it on each, and refuses a declaration that can never run.
///
/// Each declaration's conversion is resolved from its `type`, its `transformation`, and its shape
/// (see [`Conversion::resolve`]), so the extraction stage reads one conversion per declaration that
/// is known to apply. A `geometry` block on a conversion that reads no geometry would be ignored on
/// every record, and a conversion and a target that disagree (asking for the largest member of
/// something with no extent to rank by, say) would refuse every record in turn once the run
/// started. Refusing the document instead means the mistake is reported once, by name, before a
/// single record is read.
///
/// # Errors
/// Returns [`MappingError::TransformationNotAdmitted`], [`MappingError::ShapeNotAdmitted`],
/// [`MappingError::GeometryPolicyWithoutGeometry`], or [`MappingError::InvalidAttribute`] naming the
/// first declaration that cannot run.
pub fn resolve(attributes: &mut Attributes) -> Result<()> {
    attributes.iter_mut().try_for_each(|(name, attribute)| resolve_attribute(name, attribute))
}

/// Resolves one attribute and everything nested beneath it, all reported under `name`.
fn resolve_attribute(name: &NameBuf, attribute: &mut Attribute) -> Result<()> {
    let kind = *attribute.kind();
    resolve_declaration(name, attribute, shape_of(attribute), kind)
}

/// Resolves one declaration of `shape`, reported under `name` as a declaration of `reported`, then
/// everything nested beneath it.
///
/// The four nestings that can carry a further declaration are all walked: an object attribute's
/// `mappings`, a `LanguageProperty`'s per-language entries, an attribute's sub-attribute
/// `properties`, and a `syntheticEntity`'s own attributes. Instances are not walked: they share
/// their parent's `type`, `transformation`, `geometry`, and therefore its conversion.
fn resolve_declaration(name: &NameBuf, attribute: &mut Attribute, shape: DeclarationShape, reported: NgsiLdAttributeKind) -> Result<()> {
    let conversion = conversion_of(name, attribute, shape, reported)?;
    attribute.set_conversion(conversion);
    check_geometry_policy(name, attribute)?;

    resolve(attribute.mappings_mut())?;
    for entry in attribute.language_map_mut().values_mut() {
        resolve_declaration(name, entry, DeclarationShape::LanguageEntry, reported)?;
    }
    if let Some(properties) = attribute.properties_mut() {
        resolve(properties)?;
    }
    if let Some(synthetic) = attribute.synthetic_entity_mut() {
        resolve(synthetic.attributes_mut())?;
    }

    Ok(())
}

/// What an attribute is built from: its `languageMap` when it declares one, else its nested
/// `mappings` when it declares any, else its `source`.
fn shape_of(attribute: &Attribute) -> DeclarationShape {
    if !attribute.language_map().is_empty() {
        DeclarationShape::LanguageMap
    } else if !attribute.mappings().is_empty() {
        DeclarationShape::Value(ValueOrigin::Assembled)
    } else {
        DeclarationShape::Value(ValueOrigin::Sourced)
    }
}

/// The conversion of one declaration, or the load error naming why it has none.
///
/// A `languageMap` entry is reported as a declaration of the `LanguageProperty` holding it, since
/// its own `type` plays no part in what it converts.
fn conversion_of(name: &NameBuf, attribute: &Attribute, shape: DeclarationShape, reported: NgsiLdAttributeKind) -> Result<Conversion> {
    let kind = *attribute.kind();
    Conversion::resolve(kind, *attribute.transformation(), shape).map_err(|refusal| match refusal {
        ConversionRefusal::Transformation(transformation) => MappingError::TransformationNotAdmitted {
            attribute: name.clone(),
            kind: reported,
            shape,
            transformation,
            admitted: Conversion::admitted(kind, shape),
        },
        ConversionRefusal::Shape => MappingError::ShapeNotAdmitted {
            attribute: name.clone(),
            kind: reported,
            shape,
        },
    })
}

/// Refuses a `geometry` block that can never apply, or whose conversion can never reach the type
/// the attribute's conversion names.
fn check_geometry_policy(name: &NameBuf, attribute: &Attribute) -> Result<()> {
    let Some(policy) = attribute.geometry() else {
        return Ok(());
    };
    let target = attribute
        .conversion()
        .geometry_target()
        .ok_or_else(|| MappingError::GeometryPolicyWithoutGeometry { attribute: name.clone() })?;

    check(target, *policy.convert()).map_err(|source| MappingError::InvalidAttribute {
        attribute: name.clone(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::Attribute,
        conversion::{
            Conversion,
            declaration_shape::{DeclarationShape, ValueOrigin},
            json_conversion::JsonConversion,
            list_conversion::ListConversion,
            property_conversion::PropertyConversion,
            value_reading::{IdentifierReading, ValueReading},
        },
        error::MappingError,
        mapping::Mapping,
        template::runner::TemplateRunner,
    };
    use cassiopeia_geometry::{geometry::GeometryKind, target::GeometryTarget};
    use cassiopeia_ngsi_ld::{
        entity::{attribute::NgsiLdAttributeKind, name::NameBuf},
        value::types::TemporalValueType,
    };
    use pretty_assertions::assert_eq;
    use std::path::Path;

    const SOURCED: DeclarationShape = DeclarationShape::Value(ValueOrigin::Sourced);
    const ASSEMBLED: DeclarationShape = DeclarationShape::Value(ValueOrigin::Assembled);

    /// The transformations a `GeoProperty` admits, as a load error quotes them.
    const GEOMETRY_TOKENS: [&str; 7] = ["geometry", "point", "multipoint", "linestring", "multilinestring", "polygon", "multipolygon"];

    fn load(attributes: &str) -> Result<Mapping, MappingError> {
        let document = format!(
            r#"{{
                version: "v4",
                dataModel: "dataModel.Environment/AirQualityObserved",
                identity: {{ entityName: "Station-{{{{ id }}}}" }},
                attributes: {attributes},
            }}"#
        );

        Mapping::from_json5(&document, Path::new("test.json5"), &mut TemplateRunner::new())
    }

    #[test]
    fn a_conversion_that_cannot_produce_the_declared_type_names_the_attribute() {
        let error = load(
            r#"{
                location: {
                    type: "GeoProperty",
                    transformation: "point",
                    geometry: { convert: "largest" },
                    source: "{{ geometry }}",
                },
            }"#,
        )
        .expect_err("the declaration cannot run");

        assert!(matches!(&error, MappingError::InvalidAttribute { attribute, .. } if attribute.as_str() == "location"));
        assert!(error.to_string().contains("location"));
    }

    #[test]
    fn a_conversion_matching_its_declared_type_loads() {
        assert!(
            load(
                r#"{
                location: {
                    type: "GeoProperty",
                    transformation: "polygon",
                    geometry: { convert: "largest" },
                    source: "{{ geometry }}",
                },
            }"#,
            )
            .is_ok()
        );
    }

    #[test]
    fn a_conversion_inside_a_synthetic_entity_is_checked_too() {
        let error = load(
            r#"{
                centroid: {
                    type: "Relationship",
                    source: "{{ id }}",
                    target: { entity: "Place" },
                    syntheticEntity: {
                        dataModel: "Place",
                        identity: { entityName: "Place-{{ id }}" },
                        attributes: {
                            pin: {
                                type: "GeoProperty",
                                transformation: "point",
                                geometry: { convert: "convex-hull" },
                                source: "{{ geometry }}",
                            },
                        },
                    },
                },
            }"#,
        )
        .expect_err("the nested declaration cannot run");

        assert!(matches!(&error, MappingError::InvalidAttribute { attribute, .. } if attribute.as_str() == "pin"));
    }

    #[test]
    fn an_attribute_declaring_no_geometry_block_is_left_alone() {
        assert!(
            load(
                r#"{
                temperature: { source: "{{ temperature }}", transformation: "float" },
            }"#,
            )
            .is_ok()
        );
    }

    /// Asserts the declaration was refused because its `geometry` block names no geometry to apply
    /// to, naming `expected` as the attribute.
    fn assert_refused_as_policy_without_geometry(result: Result<Mapping, MappingError>, expected: &str) {
        let error = result.expect_err("the geometry block can never apply");

        assert!(
            matches!(&error, MappingError::GeometryPolicyWithoutGeometry { attribute } if attribute.as_str() == expected),
            "{error:?}"
        );
        assert!(error.to_string().contains(expected));
    }

    #[test]
    fn a_geometry_block_on_a_geo_property_without_a_transformation_loads_under_the_preserving_default() {
        for convert in ["largest", "centroid", "flatten", "envelope"] {
            let result = load(&format!(
                r#"{{
                    location: {{ type: "GeoProperty", geometry: {{ convert: "{convert}" }}, source: "{{{{ geometry }}}}" }},
                }}"#
            ));

            assert!(result.is_ok(), "{convert}: {result:?}");
        }
    }

    #[test]
    fn a_geometry_block_on_a_property_without_a_transformation_is_refused_naming_the_attribute() {
        assert_refused_as_policy_without_geometry(
            load(
                r#"{
                    area: { source: "{{ geometry }}", geometry: { convert: "centroid" } },
                }"#,
            ),
            "area",
        );
    }

    #[test]
    fn a_geo_property_under_a_string_transformation_and_a_geometry_block_is_refused_for_its_transformation() {
        assert_refused_as_geo_property_without_geometry(
            load(
                r#"{
                    location: { type: "GeoProperty", transformation: "string", geometry: { altitude: "drop" }, source: "{{ geometry }}" },
                }"#,
            ),
            "location",
            "string",
        );
    }

    /// Asserts the declaration was refused because its type, a `kind` of `shape`, cannot take
    /// `transformation`, naming `expected` as the attribute and offering each of `admitted`.
    fn assert_not_admitted(
        result: Result<Mapping, MappingError>,
        expected: &str,
        (kind, shape): (NgsiLdAttributeKind, DeclarationShape),
        transformation: &str,
        admitted: &[&str],
    ) {
        let error = result.expect_err("the transformation cannot apply");

        assert!(
            matches!(
                &error,
                MappingError::TransformationNotAdmitted { attribute, kind: refused_kind, shape: refused_shape, transformation: declared, .. }
                    if attribute.as_str() == expected && *refused_kind == kind && *refused_shape == shape && declared.to_string() == transformation
            ),
            "{error:?}"
        );
        let message = error.to_string();
        assert!(message.contains(&format!("`{expected}` ({kind}, {shape})")), "{message}");
        assert!(message.contains(&format!(r#"transformation: "{transformation}""#)), "{message}");
        assert!(message.contains("remove the transformation"), "{message}");
        if admitted.is_empty() {
            assert!(message.contains("takes none"), "{message}");
        }
        for token in admitted {
            assert!(message.contains(&format!(r#""{token}""#)), "{message} lacks {token}");
        }
    }

    /// Asserts the declaration was refused because it is a `GeoProperty` read from a source whose
    /// `transformation` produces no geometry, naming `expected` as the attribute.
    fn assert_refused_as_geo_property_without_geometry(result: Result<Mapping, MappingError>, expected: &str, transformation: &str) {
        assert_not_admitted(result, expected, (NgsiLdAttributeKind::GeoProperty, SOURCED), transformation, &GEOMETRY_TOKENS);
    }

    #[test]
    fn a_geo_property_under_a_transformation_producing_no_geometry_is_refused_naming_the_attribute() {
        for transformation in ["string", "integer", "object"] {
            assert_refused_as_geo_property_without_geometry(
                load(&format!(
                    r#"{{
                        location: {{ type: "GeoProperty", transformation: "{transformation}", source: "{{{{ geometry }}}}" }},
                    }}"#
                )),
                "location",
                transformation,
            );
        }
    }

    #[test]
    fn a_geo_property_sub_attribute_under_a_transformation_producing_no_geometry_is_refused_naming_it() {
        for transformation in ["string", "integer", "object"] {
            assert_refused_as_geo_property_without_geometry(
                load(&format!(
                    r#"{{
                        name: {{
                            source: "{{{{ name }}}}",
                            properties: {{ footprint: {{ type: "GeoProperty", transformation: "{transformation}", source: "{{{{ geometry }}}}" }} }},
                        }},
                    }}"#
                )),
                "footprint",
                transformation,
            );
        }
    }

    #[test]
    fn a_synthetic_entity_geo_property_under_a_transformation_producing_no_geometry_is_refused_naming_it() {
        for transformation in ["string", "integer", "object"] {
            assert_refused_as_geo_property_without_geometry(
                load(&format!(
                    r#"{{
                        place: {{
                            type: "Relationship",
                            source: "{{{{ id }}}}",
                            target: {{ entity: "Place" }},
                            syntheticEntity: {{
                                dataModel: "Place",
                                identity: {{ entityName: "Place-{{{{ id }}}}" }},
                                attributes: {{
                                    pin: {{ type: "GeoProperty", transformation: "{transformation}", source: "{{{{ geometry }}}}" }},
                                }},
                            }},
                        }},
                    }}"#
                )),
                "pin",
                transformation,
            );
        }
    }

    #[test]
    fn a_geo_property_without_a_transformation_or_under_a_geometry_transformation_loads() {
        for declaration in [
            r#"type: "GeoProperty", source: "{{ geometry }}""#,
            r#"type: "GeoProperty", transformation: "geometry", source: "{{ geometry }}""#,
            r#"type: "GeoProperty", transformation: "point", source: ["{{ lon }}", "{{ lat }}"]"#,
        ] {
            let result = load(&format!(
                r#"{{
                    location: {{ {declaration} }},
                    name: {{ source: "{{{{ name }}}}", properties: {{ footprint: {{ {declaration} }} }} }},
                }}"#
            ));

            assert!(result.is_ok(), "{declaration}: {result:?}");
        }
    }

    #[test]
    fn a_property_under_a_transformation_producing_no_geometry_is_left_alone() {
        assert!(
            load(
                r#"{
                    label: { source: "{{ geometry }}", transformation: "string" },
                }"#,
            )
            .is_ok()
        );
    }

    #[test]
    fn a_geometry_block_on_a_scalar_transformation_is_refused() {
        assert_refused_as_policy_without_geometry(
            load(
                r#"{
                    temperature: { source: "{{ temperature }}", transformation: "float", geometry: { winding: "keep" } },
                }"#,
            ),
            "temperature",
        );
    }

    #[test]
    fn a_geometry_block_on_a_json_property_kept_as_read_is_refused() {
        assert_refused_as_policy_without_geometry(
            load(
                r#"{
                    payload: { type: "JsonProperty", source: "{{ geometry }}", geometry: { convert: "flatten" } },
                }"#,
            ),
            "payload",
        );
    }

    /// Asserts the declaration was refused because its transformation can never apply to the object
    /// its nested `mappings` assemble, naming `expected` as the attribute.
    fn assert_refused_as_transformation_on_assembled_object(result: Result<Mapping, MappingError>, expected: &str, transformation: &str) {
        assert_not_admitted(
            result,
            expected,
            (NgsiLdAttributeKind::Property, ASSEMBLED),
            transformation,
            &["string", "array", "object", "json", "geometry", "point"],
        );
    }

    #[test]
    fn an_attribute_with_nested_mappings_under_a_transformation_that_never_applies_to_an_object_is_refused_naming_it() {
        for transformation in ["boolean", "integer", "float", "datetime", "date", "time"] {
            assert_refused_as_transformation_on_assembled_object(
                load(&format!(
                    r#"{{
                        address: {{ transformation: "{transformation}", mappings: {{ street: {{ source: "{{{{ street }}}}" }} }} }},
                    }}"#
                )),
                "address",
                transformation,
            );
        }
    }

    #[test]
    fn a_nested_mapping_under_a_transformation_that_never_applies_to_an_object_is_refused_naming_it() {
        assert_refused_as_transformation_on_assembled_object(
            load(
                r#"{
                    address: { mappings: { street: { transformation: "integer", mappings: { name: { source: "{{ street }}" } } } } },
                }"#,
            ),
            "street",
            "integer",
        );
    }

    #[test]
    fn a_sub_attribute_with_nested_mappings_under_a_transformation_that_never_applies_to_an_object_is_refused_naming_it() {
        assert_refused_as_transformation_on_assembled_object(
            load(
                r#"{
                    name: {
                        source: "{{ name }}",
                        properties: { origin: { transformation: "float", mappings: { street: { source: "{{ street }}" } } } },
                    },
                }"#,
            ),
            "origin",
            "float",
        );
    }

    #[test]
    fn an_attribute_with_nested_mappings_under_a_transformation_that_applies_to_an_object_loads() {
        for transformation in ["object", "json", "string", "array", "geometry", "point", "multipoint"] {
            let result = load(&format!(
                r#"{{
                    address: {{ transformation: "{transformation}", mappings: {{ street: {{ source: "{{{{ street }}}}" }} }} }},
                }}"#
            ));

            assert!(result.is_ok(), "{transformation}: {result:?}");
        }
    }

    #[test]
    fn a_leaf_under_a_scalar_transformation_is_left_alone() {
        assert!(
            load(
                r#"{
                    count: { source: "{{ count }}", transformation: "integer" },
                }"#,
            )
            .is_ok()
        );
    }

    #[test]
    fn a_geo_property_with_nested_mappings_under_a_scalar_transformation_is_refused_for_producing_no_geometry() {
        assert_not_admitted(
            load(
                r#"{
                    location: { type: "GeoProperty", transformation: "integer", mappings: { type: { source: "Point" } } },
                }"#,
            ),
            "location",
            (NgsiLdAttributeKind::GeoProperty, ASSEMBLED),
            "integer",
            &GEOMETRY_TOKENS,
        );
    }

    #[test]
    fn a_geometry_block_on_a_nested_sub_attribute_without_a_geometry_conversion_is_refused() {
        assert_refused_as_policy_without_geometry(
            load(
                r#"{
                    temperature: {
                        source: "{{ temperature }}",
                        transformation: "float",
                        properties: { label: { source: "{{ name }}", geometry: { convert: "first" } } },
                    },
                }"#,
            ),
            "label",
        );
    }

    /// Asserts the declaration was refused because its type cannot be built from its shape, naming
    /// `expected` as the attribute.
    fn assert_shape_not_admitted(result: Result<Mapping, MappingError>, expected: &str, kind: NgsiLdAttributeKind, shape: DeclarationShape) {
        let error = result.expect_err("the type cannot be built from this shape");

        assert!(
            matches!(
                &error,
                MappingError::ShapeNotAdmitted { attribute, kind: refused_kind, shape: refused_shape }
                    if attribute.as_str() == expected && *refused_kind == kind && *refused_shape == shape
            ),
            "{error:?}"
        );
        assert_eq!(error.to_string(), format!("Attribute `{expected}` is a {kind}, which cannot be {shape}"));
    }

    #[test]
    fn a_relationship_declaring_a_transformation_is_refused_naming_it() {
        assert_not_admitted(
            load(r#"{ owner: { type: "Relationship", source: "{{ id }}", target: { entity: "Organization" }, transformation: "string" } }"#),
            "owner",
            (NgsiLdAttributeKind::Relationship, SOURCED),
            "string",
            &[],
        );
    }

    #[test]
    fn a_list_relationship_declaring_a_transformation_is_refused_naming_it() {
        assert_not_admitted(
            load(r#"{ lines: { type: "ListRelationship", source: "{{ lines }}", target: { entity: "Line" }, transformation: "array" } }"#),
            "lines",
            (NgsiLdAttributeKind::ListRelationship, SOURCED),
            "array",
            &[],
        );
    }

    #[test]
    fn a_json_property_under_a_scalar_transformation_is_refused_listing_what_it_admits() {
        for transformation in ["integer", "string", "datetime", "iri", "point"] {
            assert_not_admitted(
                load(&format!(
                    r#"{{ params: {{ type: "JsonProperty", source: "{{{{ params }}}}", transformation: "{transformation}" }} }}"#
                )),
                "params",
                (NgsiLdAttributeKind::JsonProperty, SOURCED),
                transformation,
                &["array", "object", "json"],
            );
        }
    }

    #[test]
    fn a_list_property_under_an_item_transformation_loads() {
        let mapping = load(
            r#"{
                integers: { type: "ListProperty", source: "{{ codes }}", transformation: "integer" },
                texts: { type: "ListProperty", source: "{{ codes }}", transformation: "string" },
                objects: { type: "ListProperty", source: "{{ codes }}", transformation: "object" },
                decoded: { type: "ListProperty", source: "{{ codes }}", transformation: "json" },
                links: { type: "ListProperty", source: "{{ links }}", transformation: "iri" },
                pins: { type: "ListProperty", source: "{{ pins }}", transformation: "point", geometry: { altitude: "drop" } },
            }"#,
        )
        .unwrap();

        for (name, reading) in [
            ("integers", ValueReading::Integer),
            ("texts", ValueReading::Text),
            ("objects", ValueReading::Object),
            ("decoded", ValueReading::Json),
            ("links", ValueReading::Identifier(IdentifierReading::Iri)),
            ("pins", ValueReading::Geometry(GeometryTarget::Coerce(GeometryKind::Point))),
        ] {
            assert_eq!(
                attribute(&mapping, name).conversion(),
                Conversion::ListProperty(ListConversion::Each(reading)),
                "{name}"
            );
        }
    }

    #[test]
    fn a_list_property_built_from_nested_mappings_under_a_reading_that_never_applies_to_an_object_is_refused() {
        assert_not_admitted(
            load(r#"{ codes: { type: "ListProperty", transformation: "integer", mappings: { code: { source: "{{ code }}" } } } }"#),
            "codes",
            (NgsiLdAttributeKind::ListProperty, ASSEMBLED),
            "integer",
            &[
                "string",
                "array",
                "object",
                "json",
                "geometry",
                "point",
                "multipoint",
                "linestring",
                "multilinestring",
                "polygon",
                "multipolygon",
            ],
        );
    }

    #[test]
    fn a_geometry_block_on_a_list_property_reading_no_geometry_is_refused() {
        assert_refused_as_policy_without_geometry(
            load(r#"{ codes: { type: "ListProperty", source: "{{ codes }}", transformation: "integer", geometry: { convert: "first" } } }"#),
            "codes",
        );
    }

    #[test]
    fn a_vocab_property_under_string_is_refused_listing_iri_and_urn() {
        for transformation in ["string", "integer"] {
            assert_not_admitted(
                load(&format!(
                    r#"{{ level: {{ type: "VocabProperty", source: "{{{{ level }}}}", transformation: "{transformation}" }} }}"#
                )),
                "level",
                (NgsiLdAttributeKind::VocabProperty, SOURCED),
                transformation,
                &["iri", "urn"],
            );
        }
    }

    #[test]
    fn a_vocab_property_under_iri_or_urn_loads_with_its_identifier_reading() {
        let mapping = load(
            r#"{
                level: { type: "VocabProperty", source: "{{ level }}", transformation: "iri" },
                kind: { type: "VocabProperty", source: "{{ kind }}", transformation: "urn" },
            }"#,
        )
        .unwrap();

        assert_eq!(attribute(&mapping, "level").conversion(), Conversion::VocabProperty(IdentifierReading::Iri));
        assert_eq!(attribute(&mapping, "kind").conversion(), Conversion::VocabProperty(IdentifierReading::Urn));
    }

    #[test]
    fn a_language_property_with_a_language_map_declaring_a_transformation_is_refused() {
        assert_not_admitted(
            load(r#"{ label: { type: "LanguageProperty", transformation: "object", languageMap: { en: { source: "{{ en }}" } } } }"#),
            "label",
            (NgsiLdAttributeKind::LanguageProperty, DeclarationShape::LanguageMap),
            "object",
            &[],
        );
    }

    #[test]
    fn a_language_map_entry_under_anything_but_string_is_refused_naming_the_attribute() {
        for transformation in ["integer", "datetime", "object"] {
            assert_not_admitted(
                load(&format!(
                    r#"{{ label: {{ type: "LanguageProperty", languageMap: {{ en: {{ source: "{{{{ en }}}}", transformation: "{transformation}" }} }} }} }}"#
                )),
                "label",
                (NgsiLdAttributeKind::LanguageProperty, DeclarationShape::LanguageEntry),
                transformation,
                &["string"],
            );
        }
    }

    #[test]
    fn a_language_property_read_from_one_source_under_anything_but_object_is_refused() {
        assert_not_admitted(
            load(r#"{ name: { type: "LanguageProperty", source: "{{ name }}", transformation: "string" } }"#),
            "name",
            (NgsiLdAttributeKind::LanguageProperty, SOURCED),
            "string",
            &["object"],
        );
    }

    #[test]
    fn a_vocab_property_built_from_nested_mappings_is_refused() {
        assert_shape_not_admitted(
            load(r#"{ level: { type: "VocabProperty", mappings: { term: { source: "{{ term }}" } } } }"#),
            "level",
            NgsiLdAttributeKind::VocabProperty,
            ASSEMBLED,
        );
    }

    #[test]
    fn a_relationship_built_from_nested_mappings_is_refused() {
        assert_shape_not_admitted(
            load(r#"{ owner: { type: "Relationship", target: { entity: "Organization" }, mappings: { id: { source: "{{ id }}" } } } }"#),
            "owner",
            NgsiLdAttributeKind::Relationship,
            ASSEMBLED,
        );
    }

    #[test]
    fn a_language_map_on_any_type_but_a_language_property_is_refused() {
        assert_shape_not_admitted(
            load(r#"{ label: { languageMap: { en: { source: "{{ en }}" } } } }"#),
            "label",
            NgsiLdAttributeKind::Property,
            DeclarationShape::LanguageMap,
        );
    }

    #[test]
    fn a_sub_attribute_under_a_transformation_its_type_does_not_admit_is_refused_naming_it() {
        assert_not_admitted(
            load(
                r#"{
                    temperature: {
                        source: "{{ t }}",
                        transformation: "float",
                        properties: { level: { type: "VocabProperty", source: "{{ l }}", transformation: "integer" } },
                    },
                }"#,
            ),
            "level",
            (NgsiLdAttributeKind::VocabProperty, SOURCED),
            "integer",
            &["iri", "urn"],
        );
    }

    #[test]
    fn a_synthetic_entity_attribute_under_a_transformation_its_type_does_not_admit_is_refused_naming_it() {
        assert_not_admitted(
            load(
                r#"{
                    place: {
                        type: "Relationship",
                        source: "{{ id }}",
                        target: { entity: "Place" },
                        syntheticEntity: {
                            dataModel: "Place",
                            identity: { entityName: "Place-{{ id }}" },
                            attributes: { payload: { type: "JsonProperty", source: "{{ p }}", transformation: "float" } },
                        },
                    },
                }"#,
            ),
            "payload",
            (NgsiLdAttributeKind::JsonProperty, SOURCED),
            "float",
            &["array", "object", "json"],
        );
    }

    #[test]
    fn a_property_under_a_geometry_transformation_loads() {
        let result = load(
            r#"{
                shape: { source: "{{ shape }}", transformation: "geometry" },
                pin: { source: ["{{ lon }}", "{{ lat }}"], transformation: "point" },
                area: { source: "{{ area }}", transformation: "geometry", geometry: { convert: "flatten" } },
            }"#,
        );

        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn every_type_and_transformation_pair_the_examples_use_loads() {
        let result = load(
            r#"{
                area: { type: "GeoProperty", source: "{{ area }}", transformation: "geometry" },
                location: { type: "GeoProperty", source: ["{{ lon }}", "{{ lat }}"], transformation: "point" },
                footprint: { type: "GeoProperty", source: "{{ footprint }}", transformation: "polygon" },
                parameters: { type: "JsonProperty", source: "{{ parameters }}", transformation: "object" },
                countdowns: { type: "ListProperty", source: "{{ countdowns }}", transformation: "array" },
                tags: { source: "{{ tags }}", transformation: "array" },
                open: { source: "{{ open }}", transformation: "boolean" },
                opened: { source: "{{ opened }}", transformation: "date" },
                observed: { source: "{{ observed }}", transformation: "datetime" },
                temperature: { source: "{{ temperature }}", transformation: "float" },
                capacity: { source: "{{ capacity }}", transformation: "integer" },
                address: { source: "{{ address }}", transformation: "object" },
                name: { source: "{{ name }}", transformation: "string" },
            }"#,
        );

        assert!(result.is_ok(), "{result:?}");
    }

    /// The loaded declaration of the top-level attribute `name`.
    fn attribute<'a>(mapping: &'a Mapping, name: &str) -> &'a Attribute {
        mapping.attributes().get(&NameBuf::new(name).unwrap()).expect("attribute declared")
    }

    #[test]
    fn the_load_pass_stores_each_declaration_s_resolved_conversion() {
        let mapping = load(
            r#"{
                temperature: {
                    source: "{{ t }}",
                    transformation: "float",
                    properties: { observedAt: { source: "{{ ts }}", transformation: "datetime" } },
                },
                label: { type: "LanguageProperty", languageMap: { en: { source: "{{ en }}" } } },
                reading: { transformation: "integer", instances: [ { source: "{{ a }}" }, { source: "{{ b }}" } ] },
                address: { mappings: { city: { source: "{{ city }}", transformation: "string" } } },
            }"#,
        )
        .unwrap();

        let temperature = attribute(&mapping, "temperature");
        assert_eq!(temperature.conversion(), Conversion::Property(PropertyConversion::One(ValueReading::Float)));
        let observed_at = temperature
            .properties()
            .as_ref()
            .and_then(|properties| properties.get(&NameBuf::new("observedAt").unwrap()));
        assert_eq!(
            observed_at.map(Attribute::conversion),
            Some(Conversion::Property(PropertyConversion::One(ValueReading::Temporal(
                TemporalValueType::DateTime
            ))))
        );
        let label = attribute(&mapping, "label");
        assert_eq!(label.conversion(), Conversion::LanguageProperty);
        assert_eq!(
            label.language_map().values().map(Attribute::conversion).collect::<Vec<_>>(),
            [Conversion::LanguageText]
        );
        assert_eq!(
            attribute(&mapping, "reading").conversion(),
            Conversion::Property(PropertyConversion::One(ValueReading::Integer))
        );
        let address = attribute(&mapping, "address");
        assert_eq!(address.conversion(), Conversion::Property(PropertyConversion::One(ValueReading::Object)));
        assert_eq!(
            address.mappings().values().map(Attribute::conversion).collect::<Vec<_>>(),
            [Conversion::Property(PropertyConversion::One(ValueReading::Text))]
        );
    }

    #[test]
    fn each_type_without_a_transformation_loads_with_its_default_conversion() {
        let mapping = load(
            r#"{
                codes: { type: "ListProperty", source: "{{ codes }}" },
                payload: { type: "JsonProperty", source: "{{ payload }}" },
                name: { source: "{{ name }}" },
                location: { type: "GeoProperty", source: "{{ geometry }}" },
                title: { type: "LanguageProperty", source: "{{ title }}" },
                category: { type: "VocabProperty", source: "{{ category }}" },
                owner: { type: "Relationship", source: "{{ owner }}", target: { entity: "Organization" } },
            }"#,
        )
        .unwrap();

        for (name, expected) in [
            ("codes", Conversion::ListProperty(ListConversion::AsRead)),
            ("payload", Conversion::JsonProperty(JsonConversion::Verbatim)),
            ("name", Conversion::Property(PropertyConversion::One(ValueReading::Text))),
            ("location", Conversion::GeoProperty(GeometryTarget::Preserve)),
            ("title", Conversion::LanguageProperty),
            ("category", Conversion::VocabProperty(IdentifierReading::Iri)),
            ("owner", Conversion::Relationship),
        ] {
            assert_eq!(attribute(&mapping, name).conversion(), expected, "{name}");
        }
    }

    #[test]
    fn a_declared_transformation_loads_as_the_conversion_it_names() {
        let mapping = load(
            r#"{
                payload: { type: "JsonProperty", source: "{{ payload }}", transformation: "object" },
                raw: { type: "JsonProperty", source: "{{ raw }}", transformation: "json" },
                location: { type: "GeoProperty", source: "{{ geometry }}", transformation: "point" },
                address: { transformation: "string", mappings: { city: { source: "{{ city }}" } } },
            }"#,
        )
        .unwrap();

        for (name, expected) in [
            ("payload", Conversion::JsonProperty(JsonConversion::Object)),
            ("raw", Conversion::JsonProperty(JsonConversion::Json)),
            ("location", Conversion::GeoProperty(GeometryTarget::Coerce(GeometryKind::Point))),
            ("address", Conversion::Property(PropertyConversion::One(ValueReading::Text))),
        ] {
            assert_eq!(attribute(&mapping, name).conversion(), expected, "{name}");
        }
    }
}
