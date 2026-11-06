use crate::{
    attribute::{
        Attribute,
        Attributes,
        declaration_trail::{DeclarationTrail, Nesting},
    },
    error::{MappingError, Result},
};
use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};

/// The member name an attribute's datatype is written under (ETSI GS CIM 009 v1.9.1 clause 4.5.2).
const VALUE_TYPE: &str = "valueType";

/// Checks, at load time, that every `valueType` declared in `attributes` sits where one can be
/// written.
///
/// A `valueType` is a member of a Property or one of its subclasses (ETSI GS CIM 009 v1.9.1 clause
/// 4.5.2, tables 5.2.5-1, 5.2.7-1, 5.2.32-1, 5.2.35-1, 5.2.36-1 and 5.2.38-1), so three places are
/// refused: a Relationship or `ListRelationship` and its instances, which have none (tables 5.2.6-1
/// and 5.2.33-1); a sub-attribute named `valueType` under `properties`, the attribute's own or an
/// instance's, which would be written as a nested Property a broker refuses; and an entry of an
/// attribute's `mappings` or `languageMap`, which builds part of a value rather than an attribute.
/// Whether a declared `valueType` agrees with the attribute's transformation depends on the
/// `@context` it expands under, so that is checked once the run knows it (see
/// [`check_value_types`](crate::value_type_agreement::check_value_types)).
///
/// # Errors
/// Returns [`MappingError::ValueTypeOnRelationship`], [`MappingError::ValueTypeUnderProperties`], or
/// [`MappingError::ValueTypeOnValueMember`] naming the first declaration that declares a `valueType`
/// where none can be written.
pub fn check(attributes: &Attributes) -> Result<()> {
    attributes.iter().try_for_each(|(name, attribute)| check_attribute(None, name, attribute))
}

/// Checks one declaration, which sits at `name` below the declarations `owners` holds, then its
/// sub-attributes and its synthetic entity's attributes.
fn check_attribute(owners: Option<&DeclarationTrail<'_>>, name: &NameBuf, attribute: &Attribute) -> Result<()> {
    if let kind @ (NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship) = *attribute.kind() {
        let instance_declares = attribute.instances().iter().flatten().any(|instance| instance.value_type().is_some());
        if attribute.value_type().is_some() || instance_declares {
            return Err(MappingError::ValueTypeOnRelationship {
                attribute: DeclarationTrail::path_of(owners, name),
                kind,
            });
        }
    }
    let value_members = attribute.mappings().values().chain(attribute.language_map().values());
    if value_members.flat_map(Attribute::iter_recursive).any(|member| member.value_type().is_some()) {
        return Err(MappingError::ValueTypeOnValueMember {
            attribute: DeclarationTrail::path_of(owners, name),
        });
    }

    // An instance's own `properties` are sub-attributes of the attribute, exactly as the
    // attribute-level ones are, so both are walked beneath the same frame.
    let qualified = DeclarationTrail::below(name, Nesting::SubAttribute, owners);
    let instance_properties = attribute.instances().iter().flatten().filter_map(|instance| instance.properties().as_ref());
    for properties in attribute.properties().iter().chain(instance_properties) {
        if properties.contains_key(VALUE_TYPE) {
            return Err(MappingError::ValueTypeUnderProperties {
                attribute: DeclarationTrail::path_of(owners, name),
            });
        }
        properties
            .iter()
            .try_for_each(|(sub_name, sub_attribute)| check_attribute(Some(&qualified), sub_name, sub_attribute))?;
    }

    attribute.synthetic_entity().as_ref().map_or(Ok(()), |synthetic| check(synthetic.attributes()))
}

#[cfg(test)]
mod tests {
    use crate::{error::MappingError, mapping::Mapping, template::runner::TemplateRunner};
    use pretty_assertions::assert_eq;
    use std::path::Path;

    fn load(attributes: &str) -> Result<Mapping, MappingError> {
        let document = format!(
            r#"{{
                version: "v4",
                dataModel: "Place",
                identity: {{ entityName: "Place-{{{{ id }}}}" }},
                attributes: {attributes},
            }}"#
        );

        Mapping::from_json5(&document, Path::new("test.json5"), &mut TemplateRunner::new())
    }

    #[test]
    fn a_value_type_on_every_property_kind_loads() {
        for kind in ["Property", "GeoProperty", "VocabProperty", "ListProperty", "JsonProperty", "LanguageProperty"] {
            let attributes = format!(r#"{{ address: {{ type: "{kind}", source: "{{{{ address }}}}", valueType: "PostalAddress" }} }}"#);

            assert!(load(&attributes).is_ok(), "{kind}");
        }
    }

    #[test]
    fn a_value_type_on_a_relationship_is_refused_pointing_at_object_type() {
        let error = load(r#"{ owner: { type: "Relationship", source: "{{ owner }}", target: { entity: "Person" }, valueType: "Person" } }"#)
            .expect_err("a relationship has no valueType");

        assert!(
            matches!(&error, MappingError::ValueTypeOnRelationship { attribute, .. } if attribute.to_string() == "owner"),
            "{error:?}"
        );
        assert!(error.to_string().contains("objectType"));
        assert!(error.to_string().contains("`Relationship`"));
    }

    #[test]
    fn a_value_type_on_an_instance_of_a_list_relationship_is_refused() {
        let error = load(
            r#"{
                members: {
                    type: "ListRelationship",
                    target: { entity: "Person" },
                    instances: [{ source: "{{ a }}", valueType: "Person" }],
                },
            }"#,
        )
        .expect_err("a list relationship instance has no valueType");

        assert!(matches!(&error, MappingError::ValueTypeOnRelationship { attribute, .. } if attribute.to_string() == "members"));
    }

    #[test]
    fn a_value_type_under_properties_is_refused_with_a_hint_to_move_it_up() {
        let error = load(r#"{ address: { source: "{{ address }}", properties: { valueType: { source: "PostalAddress" } } } }"#)
            .expect_err("valueType is no sub-attribute");

        assert!(matches!(&error, MappingError::ValueTypeUnderProperties { attribute } if attribute.to_string() == "address"));
        assert!(error.to_string().contains("move it up a level"));
    }

    #[test]
    fn a_value_type_under_an_instance_s_own_properties_is_refused_naming_the_attribute() {
        let error = load(
            r#"{
                address: {
                    instances: [{ source: "{{ a }}", properties: { valueType: { source: "PostalAddress" } } }],
                },
            }"#,
        )
        .expect_err("an instance's properties are sub-attributes too");

        assert!(matches!(&error, MappingError::ValueTypeUnderProperties { attribute } if attribute.to_string() == "address"));
    }

    #[test]
    fn a_value_type_under_the_properties_of_a_sub_attribute_is_refused_naming_its_path() {
        let error =
            load(r#"{ address: { source: "{{ a }}", properties: { formatted: { source: "{{ f }}", properties: { valueType: { source: "String" } } } } } }"#)
                .expect_err("valueType is no sub-attribute at any depth");

        assert_eq!(
            error.to_string(),
            "Attribute `address.formatted` declares `valueType` under `properties`, where it would become a sub-attribute; move it up a level, next to `type`"
        );
    }

    #[test]
    fn a_value_type_on_a_sub_attribute_loads() {
        assert!(load(r#"{ address: { source: "{{ address }}", properties: { formatted: { source: "{{ text }}", valueType: "String" } } } }"#).is_ok());
    }

    #[test]
    fn a_value_type_on_a_sub_attribute_of_an_instance_loads() {
        assert!(load(r#"{ address: { instances: [{ source: "{{ a }}", properties: { formatted: { source: "{{ f }}", valueType: "String" } } }] } }"#).is_ok());
    }

    #[test]
    fn a_value_type_on_a_relationship_under_an_instance_s_properties_is_refused_naming_its_path() {
        let error = load(
            r#"{
                address: {
                    instances: [{
                        source: "{{ a }}",
                        properties: { verifiedBy: { type: "Relationship", source: "{{ v }}", target: { entity: "Person" }, valueType: "Person" } },
                    }],
                },
            }"#,
        )
        .expect_err("a relationship has no valueType wherever it sits");

        assert!(matches!(&error, MappingError::ValueTypeOnRelationship { attribute, .. } if attribute.to_string() == "address.verifiedBy"));
    }

    #[test]
    fn a_value_type_on_a_nested_mapping_entry_is_refused() {
        let error = load(r#"{ address: { mappings: { city: { source: "{{ city }}", valueType: "String" } } } }"#).expect_err("a value member has no valueType");

        assert!(matches!(&error, MappingError::ValueTypeOnValueMember { attribute } if attribute.to_string() == "address"));
    }

    #[test]
    fn a_value_type_on_a_language_map_entry_is_refused() {
        let error = load(r#"{ name: { type: "LanguageProperty", languageMap: { en: { source: "{{ en }}", valueType: "langString" } } } }"#)
            .expect_err("a language map entry has no valueType");

        assert!(matches!(&error, MappingError::ValueTypeOnValueMember { attribute } if attribute.to_string() == "name"));
    }

    #[test]
    fn a_misplaced_value_type_inside_a_synthetic_entity_is_refused_too() {
        let error = load(
            r#"{
                site: {
                    type: "Relationship",
                    source: "{{ id }}",
                    target: { entity: "Site" },
                    syntheticEntity: {
                        dataModel: "Site",
                        identity: { entityName: "Site-{{ id }}" },
                        attributes: { operator: { type: "Relationship", source: "{{ op }}", target: { entity: "Org" }, valueType: "Org" } },
                    },
                },
            }"#,
        )
        .expect_err("the nested relationship has no valueType");

        assert!(matches!(&error, MappingError::ValueTypeOnRelationship { attribute, .. } if attribute.to_string() == "operator"));
    }
}
