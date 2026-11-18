use crate::{
    attribute::Attributes,
    declaration_site::DeclaringEntity,
    error::{MappingError, Result},
};
use cassiopeia_ngsi_ld::entity::{name::NameBuf, reserved_member::ReservedName};

/// Refuses an attribute of `attributes`, or of a `syntheticEntity` one of them declares, that takes a
/// name NGSI-LD reserves (ETSI GS CIM 009 v1.9.1 clause 4.5.1).
///
/// Only the attributes of an entity are checked. Under an attribute's `properties`, `datasetId`,
/// `observedAt` and `unitCode` declare the attribute's own members, and a member under `mappings` is
/// a member of a value, beside no member of the entity.
///
/// # Errors
/// Returns [`MappingError::ReservedAttributeName`] naming the first attribute whose name is reserved.
pub(crate) fn check(attributes: &Attributes) -> Result<()> {
    check_entity(None, attributes)?;
    attributes.iter().try_for_each(|(owner, attribute)| {
        attribute
            .synthetic_entity()
            .as_ref()
            .map_or(Ok(()), |synthetic| check_entity(Some(owner), synthetic.attributes()))
    })
}

/// Refuses the first of `attributes` whose name is reserved: the attributes of the `syntheticEntity`
/// the attribute `owner` declares, or else of the document's own entity.
fn check_entity(owner: Option<&NameBuf>, attributes: &Attributes) -> Result<()> {
    let Some((attribute, reserved)) = attributes.keys().find_map(|name| ReservedName::of(name).map(|reserved| (name, reserved))) else {
        return Ok(());
    };

    // The error outlives the document it describes, so it owns the names it carries.
    let entity = owner.map_or(DeclaringEntity::Main, |owner| DeclaringEntity::Synthetic(owner.clone()));
    Err(MappingError::ReservedAttributeName {
        attribute: attribute.clone(),
        entity,
        reserved,
    })
}

#[cfg(test)]
mod tests {
    use crate::{declaration_site::DeclaringEntity, error::MappingError, mapping::Mapping, template::runner::TemplateRunner};
    use pretty_assertions::assert_eq;
    use std::{error::Error, path::Path};

    /// Every name the entity members and the core context's non-reified Properties reserve.
    const RESERVED: [&str; 14] = [
        "id",
        "type",
        "scope",
        "expiresAt",
        "datasetId",
        "instanceId",
        "observedAt",
        "createdAt",
        "modifiedAt",
        "deletedAt",
        "lastUsedAt",
        "unitCode",
        "valueType",
        "lang",
    ];

    fn load(document: &str) -> Result<Mapping, MappingError> {
        Mapping::from_json5(document, Path::new("device.json5"), &mut TemplateRunner::new())
    }

    /// A device mapping declaring `attributes` as its own.
    fn device(attributes: &str) -> String {
        format!(
            r#"{{
                version: "v4",
                dataModel: "Device",
                identity: {{ entityName: "D-{{{{ code }}}}" }},
                attributes: {attributes},
            }}"#
        )
    }

    /// A device mapping whose `owner` relationship declares a synthetic entity with `attributes`.
    fn device_with_owner(attributes: &str) -> String {
        device(&format!(
            r#"{{
                owner: {{
                    type: "Relationship",
                    source: "{{{{ owner }}}}",
                    target: {{ entity: "Organization" }},
                    syntheticEntity: {{
                        dataModel: "Organization",
                        identity: {{ entityName: "{{{{ owner }}}}" }},
                        attributes: {attributes},
                    }},
                }},
            }}"#
        ))
    }

    #[test]
    fn a_mapping_naming_attributes_id_type_and_scope_is_refused_naming_id() {
        let document = r#"{
            version: "v4",
            dataModel: "Device",
            identity: {
                entityName: "D-{{ code }}",
            },
            scope: "Wien",
            attributes: {
                id: { source: "{{ code }}" },
                type: { source: "{{ kind }}" },
                scope: { source: "{{ kind }}" },
            },
        }"#;

        let error = load(document).expect_err("an attribute named `id` is refused");

        assert!(
            matches!(&error, MappingError::ReservedAttributeName { attribute, entity: DeclaringEntity::Main, .. } if attribute.as_str() == "id"),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            "Attribute `id` takes a name NGSI-LD reserves: every entity carries a member named `id` (ETSI GS CIM 009 v1.9.1 clause 4.5.1), so rename the attribute"
        );
    }

    #[test]
    fn each_reserved_name_is_refused_as_an_attribute_of_the_entity() {
        for name in RESERVED {
            let error = load(&device(&format!(r#"{{ {name}: {{ source: "{{{{ code }}}}" }} }}"#))).expect_err(name);

            assert!(
                matches!(&error, MappingError::ReservedAttributeName { attribute, entity: DeclaringEntity::Main, .. } if attribute.as_str() == name),
                "{name}: {error:?}"
            );
            let message = error.to_string();
            assert!(message.starts_with(&format!("Attribute `{name}` takes a name NGSI-LD reserves")), "{message}");
            assert!(message.contains("clause 4.5.1"), "{message}");
        }
    }

    #[test]
    fn each_reserved_name_is_refused_as_an_attribute_of_a_synthetic_entity() {
        for name in RESERVED {
            let error = load(&device_with_owner(&format!(r#"{{ {name}: {{ source: "{{{{ owner }}}}" }} }}"#))).expect_err(name);

            assert!(
                matches!(
                    &error,
                    MappingError::ReservedAttributeName { attribute, entity: DeclaringEntity::Synthetic(owner), .. }
                        if attribute.as_str() == name && owner.as_str() == "owner"
                ),
                "{name}: {error:?}"
            );
            let message = error.to_string();
            assert!(
                message.starts_with(&format!("Attribute `{name}` in the syntheticEntity of `owner` takes a name NGSI-LD reserves")),
                "{message}"
            );
            assert!(message.contains("clause 4.5.1"), "{message}");
        }
    }

    #[test]
    fn a_non_reified_property_is_refused_naming_the_core_context() {
        let error = load(&device_with_owner(r#"{ observedAt: { source: "{{ seen }}" } }"#)).expect_err("observedAt is reserved");

        assert_eq!(
            error.to_string(),
            "Attribute `observedAt` in the syntheticEntity of `owner` takes a name NGSI-LD reserves: the core context defines `observedAt` as a non-reified Property (ETSI GS CIM 009 v1.9.1 clause 4.5.1), so rename the attribute"
        );
    }

    #[test]
    fn a_name_no_member_or_non_reified_property_takes_loads() {
        let attributes = r#"{ name: { source: "{{ name }}" }, location: { source: "{{ place }}" }, status: { source: "{{ state }}" } }"#;

        assert_eq!(load(&device(attributes)).expect("the names are free").attributes().len(), 3);
        assert!(load(&device_with_owner(attributes)).is_ok());
    }

    #[test]
    fn the_qualifiers_under_properties_and_the_members_under_mappings_load() {
        let attributes = r#"{
            temperature: {
                source: "{{ t }}",
                properties: {
                    observedAt: { source: "{{ seen }}" },
                    unitCode: { source: "CEL" },
                    datasetId: { source: "urn:ngsi-ld:Dataset:raw" },
                },
            },
            label: { mappings: { id: { source: "{{ code }}" }, type: { source: "{{ kind }}" } } },
        }"#;

        assert!(load(&device(attributes)).is_ok());
    }

    #[test]
    fn an_attribute_named_context_is_still_refused_as_an_illegal_name_under_clause_4_6_2() {
        let error = load(&device(r#"{ "@context": { source: "{{ code }}" } }"#)).expect_err("`@context` is no name");

        assert!(matches!(error, MappingError::Parse { .. }), "{error:?}");
        let cause = error.source().expect("the parse failure is chained").to_string();
        assert!(cause.contains("'@context' is not a legal NGSI-LD name"), "{cause}");
        assert!(cause.contains("clause 4.6.2"), "{cause}");
    }
}
