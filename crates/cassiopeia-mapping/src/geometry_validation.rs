use crate::{
    attribute::{Attribute, Attributes},
    error::{MappingError, Result},
    mapping::Mapping,
};
use cassiopeia_geometry::lattice::check;
use cassiopeia_ngsi_ld::entity::name::NameBuf;

/// Checks every attribute a mapping declares, at load time, for a `geometry` block that can never
/// run.
///
/// The block is checked against the attribute's conversion exactly as the extraction stage resolves
/// it ([`Attribute::conversion`]): the declared `transformation`, or the default of the attribute's
/// type when none is declared. A block on a conversion that reads no geometry would be ignored on
/// every record, and a conversion and a target that disagree (asking for the largest member of
/// something with no extent to rank by, say) would refuse every record in turn once the run
/// started. Refusing the document instead means the mistake is reported once, by name, before a
/// single record is read.
///
/// # Errors
/// Returns [`MappingError::GeometryPolicyWithoutGeometry`] or [`MappingError::InvalidAttribute`]
/// naming the first attribute whose declaration cannot run.
pub fn validate(mapping: &Mapping) -> Result<()> {
    validate_attributes(mapping.attributes())
}

/// Checks one level of attribute declarations and everything nested beneath each of them.
fn validate_attributes(attributes: &Attributes) -> Result<()> {
    for (name, attribute) in attributes {
        validate_attribute(name, attribute)?;
    }

    Ok(())
}

/// Checks one attribute and its nested declarations, all reported under `name`.
///
/// The four nestings that can carry a further declaration are all walked: an object attribute's
/// `mappings`, a `LanguageProperty`'s per-language entries, an attribute's sub-attribute
/// `properties`, and a `syntheticEntity`'s own attributes. Instances are not walked: they share
/// their parent's `type`, `transformation` and `geometry`, so they are already covered by it.
fn validate_attribute(name: &NameBuf, attribute: &Attribute) -> Result<()> {
    if let Some(policy) = attribute.geometry() {
        let target = attribute
            .conversion()
            .geometry_target()
            .ok_or_else(|| MappingError::GeometryPolicyWithoutGeometry { attribute: name.clone() })?;

        check(target, *policy.convert()).map_err(|source| MappingError::InvalidAttribute {
            attribute: name.clone(),
            source,
        })?;
    }

    validate_attributes(attribute.mappings())?;
    for language_entry in attribute.language_map().values() {
        validate_attribute(name, language_entry)?;
    }
    if let Some(properties) = attribute.properties() {
        validate_attributes(properties)?;
    }
    if let Some(synthetic) = attribute.synthetic_entity() {
        validate_attributes(synthetic.attributes())?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{error::MappingError, mapping::Mapping, template::runner::TemplateRunner};
    use std::path::Path;

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
    fn a_geometry_block_on_a_geo_property_under_an_explicit_string_transformation_is_refused() {
        assert_refused_as_policy_without_geometry(
            load(
                r#"{
                    location: { type: "GeoProperty", transformation: "string", geometry: { altitude: "drop" }, source: "{{ geometry }}" },
                }"#,
            ),
            "location",
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
}
