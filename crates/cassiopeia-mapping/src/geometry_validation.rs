use crate::{
    attribute::{Attribute, Attributes},
    error::{MappingError, Result},
    mapping::Mapping,
    transformation::Transformation,
};
use cassiopeia_geometry::{lattice::check, target::GeometryTarget};
use cassiopeia_ngsi_ld::entity::name::NameBuf;

/// Checks every attribute a mapping declares, at load time, for a `geometry` conversion that its
/// `transformation` can never accept.
///
/// A conversion and a target that disagree (asking for the largest member of something with no
/// extent to rank by, say) would refuse every record in turn once the run started. Refusing the
/// document instead means the mistake is reported once, by name, before a single record is read.
///
/// # Errors
/// Returns [`MappingError::InvalidAttribute`] naming the first attribute whose declaration cannot
/// run.
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
            .transformation()
            .and_then(Transformation::geometry_target)
            .unwrap_or(GeometryTarget::Preserve);

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
}
