use crate::{
    attribute::{
        Attribute,
        Attributes,
        declaration_trail::{DeclarationTrail, Nesting},
    },
    error::{MappingError, Result},
};
use cassiopeia_ngsi_ld::entity::name::NameBuf;

/// Refuses `instances` declared anywhere but on a top-level attribute of `attributes`.
///
/// Several instances of one attribute, told apart by `datasetId`, exist only on an Entity, which
/// carries them as a JSON array under the attribute's name (ETSI GS CIM 009 v1.9.1 clauses 4.5.1
/// and 4.5.5). A sub-attribute is one serialized Property or Relationship (clauses 4.5.2.2 and
/// 4.5.3.2), a member of a structured value is one JSON member, and a `languageMap` entry is one
/// localized string (clause 4.5.18), so none of them has anywhere to put a second instance. A
/// `syntheticEntity` is an entity of its own, so its top-level attributes may declare instances.
///
/// # Errors
/// Returns [`MappingError::InstancesBelowTopLevel`] or [`MappingError::InstancesInLanguageEntry`]
/// naming the first declaration that declares instances where none can be carried.
pub fn check(attributes: &Attributes) -> Result<()> {
    attributes.iter().try_for_each(|(name, attribute)| check_beneath(None, name, attribute))
}

/// Refuses `instances` on any declaration nested beneath `attribute`, which sits at `name` below
/// the declarations `owners` holds.
fn check_beneath(owners: Option<&DeclarationTrail<'_>>, name: &NameBuf, attribute: &Attribute) -> Result<()> {
    if let Some((language, _)) = attribute.language_map().iter().find(|(_, entry)| entry.instances().is_some()) {
        return Err(MappingError::InstancesInLanguageEntry {
            attribute: DeclarationTrail::path_of(owners, name),
            language: language.clone(),
        });
    }

    // An instance's own `properties` are sub-attributes of the attribute, exactly as the
    // attribute-level ones are, so both are walked beneath the same frame.
    let qualified = DeclarationTrail::below(name, Nesting::SubAttribute, owners);
    let instance_properties = attribute.instances().iter().flatten().filter_map(|instance| instance.properties().as_ref());
    attribute
        .properties()
        .iter()
        .chain(instance_properties)
        .try_for_each(|properties| check_nested(&qualified, properties))?;

    let assembled = DeclarationTrail::below(name, Nesting::Member, owners);
    check_nested(&assembled, attribute.mappings())?;

    attribute.synthetic_entity().as_ref().map_or(Ok(()), |synthetic| check(synthetic.attributes()))
}

/// Refuses each of `declarations`, nested in the declaration `owner` frames, that declares
/// `instances`, then everything nested beneath them.
fn check_nested(owner: &DeclarationTrail<'_>, declarations: &Attributes) -> Result<()> {
    declarations.iter().try_for_each(|(name, declaration)| {
        if declaration.instances().is_some() {
            return Err(MappingError::InstancesBelowTopLevel {
                attribute: DeclarationTrail::path_of(Some(owner), name),
            });
        }

        check_beneath(Some(owner), name, declaration)
    })
}

#[cfg(test)]
mod tests {
    use crate::{attribute::Attributes, error::MappingError, instance_placement::check};
    use pretty_assertions::assert_eq;

    fn attributes(document: &str) -> Attributes {
        serde_json5::from_str(document).expect("valid attributes")
    }

    fn refusal(document: &str) -> String {
        check(&attributes(document)).expect_err("the instances are refused").to_string()
    }

    const INSTANCES: &str = r#"instances: [{ source: "{{ a }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:a" } } }]"#;

    #[test]
    fn instances_on_a_top_level_attribute_and_on_a_synthetic_entity_s_top_level_attribute_pass() {
        let document = format!(
            r#"{{
                temperature: {{ {INSTANCES} }},
                owner: {{
                    syntheticEntity: {{
                        dataModel: "dataModel.Hl7/Organization",
                        identity: {{ entityName: "Owner" }},
                        attributes: {{ reading: {{ {INSTANCES} }} }},
                    }},
                }},
            }}"#
        );

        assert!(check(&attributes(&document)).is_ok());
    }

    #[test]
    fn instances_on_a_sub_attribute_are_refused_naming_its_path() {
        let document = format!(r#"{{ temperature: {{ source: "{{{{ t }}}}", properties: {{ accuracy: {{ {INSTANCES} }} }} }} }}"#);

        let error = check(&attributes(&document)).expect_err("the instances are refused");
        assert!(matches!(error, MappingError::InstancesBelowTopLevel { .. }));
        assert_eq!(
            error.to_string(),
            "Attribute `temperature.accuracy` declares `instances`, which only a top-level attribute can: several instances of one attribute exist only on an entity (ETSI GS CIM 009 v1.9.1 clause 4.5.5), so declare one `source` instead"
        );
    }

    #[test]
    fn instances_on_a_sub_attribute_of_a_sub_attribute_are_refused_naming_the_whole_path() {
        let document = format!("{{ temperature: {{ properties: {{ accuracy: {{ properties: {{ level: {{ {INSTANCES} }} }} }} }} }} }}");

        assert!(refusal(&document).starts_with("Attribute `temperature.accuracy.level` declares `instances`"));
    }

    #[test]
    fn instances_on_a_sub_attribute_of_an_instance_are_refused_naming_it_under_the_attribute() {
        let document = format!(r#"{{ temperature: {{ instances: [{{ source: "{{{{ a }}}}", properties: {{ accuracy: {{ {INSTANCES} }} }} }}] }} }}"#);

        assert!(refusal(&document).starts_with("Attribute `temperature.accuracy` declares `instances`"));
    }

    #[test]
    fn instances_on_a_member_of_a_structured_value_are_refused_naming_its_path() {
        let document = format!("{{ address: {{ mappings: {{ street: {{ {INSTANCES} }} }} }} }}");

        assert!(refusal(&document).starts_with("Attribute `address[street]` declares `instances`"));
    }

    #[test]
    fn instances_beneath_a_synthetic_entity_s_attribute_are_refused_naming_the_synthetic_path() {
        let document = format!(
            r#"{{
                owner: {{
                    syntheticEntity: {{
                        dataModel: "dataModel.Hl7/Organization",
                        identity: {{ entityName: "Owner" }},
                        attributes: {{ reading: {{ properties: {{ accuracy: {{ {INSTANCES} }} }} }} }},
                    }},
                }},
            }}"#
        );

        assert!(refusal(&document).starts_with("Attribute `reading.accuracy` declares `instances`"));
    }

    #[test]
    fn instances_on_a_language_map_entry_are_refused_naming_the_attribute_and_the_language() {
        let document =
            format!(r#"{{ name: {{ type: "LanguageProperty", languageMap: {{ en: {{ source: "{{{{ en }}}}" }}, "pt-BR": {{ {INSTANCES} }} }} }} }}"#);

        let error = check(&attributes(&document)).expect_err("the instances are refused");
        assert!(matches!(error, MappingError::InstancesInLanguageEntry { .. }));
        assert_eq!(
            error.to_string(),
            "Attribute `name` declares `instances` in its `languageMap` entry `pt-BR`, which holds one localized string (ETSI GS CIM 009 v1.9.1 clause 4.5.18); only a top-level attribute can declare instances"
        );
    }

    #[test]
    fn a_mapping_without_nested_instances_passes() {
        let document = r#"{
            temperature: { source: "{{ t }}", properties: { accuracy: { source: "{{ a }}" } } },
            address: { mappings: { street: { source: "{{ s }}" } } },
        }"#;

        assert!(check(&attributes(document)).is_ok());
    }
}
