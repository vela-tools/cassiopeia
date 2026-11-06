use crate::{
    attribute::{
        Attribute,
        Attributes,
        declaration_trail::{DeclarationTrail, Nesting},
    },
    error::{MappingError, Result},
    mapping::Mapping,
    transformation::Transformation,
};
use cassiopeia_ngsi_ld::{
    entity::{attribute::NgsiLdAttributeKind, attribute_path::AttributePath, context::ContextSource, value_type::ValueType},
    value::types::TemporalValueType,
};
use cassiopeia_value_type_expansion::{error::ExpansionError, expanded_value_types::ExpandedValueTypes, value_type_expander::ValueTypeExpander};

/// The time value data types of ETSI GS CIM 009 v1.9.1 clause 4.6.3, which a declared `valueType` may
/// name.
const TIME_DATA_TYPES: [TemporalValueType; 3] = [TemporalValueType::DateTime, TemporalValueType::Date, TemporalValueType::Time];

/// One declared `valueType` and the attribute that declares it, for itself or for one of its
/// instances.
struct Declaration<'a> {
    /// Where the attribute, or sub-attribute, the value type is declared for sits, from its
    /// top-level attribute down.
    path: AttributePath,
    /// Its declaration, whose `type` and conversion the value type has to agree with.
    attribute: &'a Attribute,
    /// The value type as declared.
    value_type: &'a ValueType,
}

/// Checks that every `valueType` a mapping and its synthetic entities declare agrees with the
/// attribute it is declared on, once expanded against the `@context` each entity is written with.
///
/// Two spellings name the same datatype only when they expand to the same IRI, so `DateTime`,
/// `ngsi-ld:DateTime` and `https://uri.etsi.org/ngsi-ld/DateTime` all name a `DateTime` under the core
/// `@context`. A `valueType` types a Property's `value` and each item of a `ListProperty`'s
/// `valueList` (clause 4.5.2, table 5.2.36-1), so a declaration agrees when:
///
/// - on a `LanguageProperty`, it names `langString`, the only value type table 5.2.32-1 admits;
/// - beside a `datetime`, `date` or `time` transformation, it names the data type every value that
///   transformation reads has (Annex C.6);
/// - naming a `DateTime`, `Date` or `Time`, the attribute reads its values through the matching
///   transformation, the only way they are sure to have the form clause 4.6.3 requires.
///
/// Each entity is expanded against the `@context` `contexts` attaches to its type, or against the core
/// `@context` alone when it attaches none. A mapping that declares no `valueType` is not expanded.
///
/// # Errors
/// Returns [`MappingError::UnexpandableValueType`] when a `@context` cannot be loaded or a value type
/// expands to no IRI, and [`MappingError::LanguagePropertyValueType`],
/// [`MappingError::ValueTypeContradictsTransformation`], or
/// [`MappingError::TimeValueTypeWithoutTransformation`] naming the first declaration that disagrees.
pub fn check_value_types(mapping: &Mapping, expander: &ValueTypeExpander, contexts: &ContextSource) -> Result<()> {
    let mut declarations = Vec::new();
    collect(None, mapping.attributes(), &mut declarations);

    if let Some(first) = declarations.first() {
        let references = TIME_DATA_TYPES.map(ValueType::from);
        let lang_string = ValueType::lang_string();
        let compared = declarations
            .iter()
            .map(|declaration| declaration.value_type)
            .chain(references.iter())
            .chain([&lang_string]);
        let context = contexts.resolve(mapping.data_model().entity_type());
        let datatypes = expander
            .expand(context, compared)
            .map_err(|source| unexpandable(&declarations, first, source))?;

        declarations.iter().try_for_each(|declaration| check(declaration, &datatypes, &lang_string))?;
    }

    mapping
        .iter_all_attributes()
        .filter_map(|attribute| attribute.synthetic_entity().as_ref())
        .try_for_each(|synthetic| check_value_types(synthetic, expander, contexts))
}

/// Gathers every `valueType` declared on one level of attributes, which sit below the declarations
/// `owners` holds, and on their sub-attributes.
///
/// An attribute with `instances` declares one per instance, its own or else the attribute's, and an
/// instance's own `properties` are sub-attributes of the attribute just as its shared ones are.
/// Nested `mappings` and `languageMap` entries declare none, which loading already ensured.
fn collect<'a>(owners: Option<&DeclarationTrail<'_>>, attributes: &'a Attributes, declarations: &mut Vec<Declaration<'a>>) {
    for (name, attribute) in attributes {
        let declaration = |value_type| Declaration {
            path: DeclarationTrail::path_of(owners, name),
            attribute,
            value_type,
        };
        match attribute.instances() {
            Some(instances) => declarations.extend(instances.iter().filter_map(|instance| attribute.instance_value_type(instance)).map(declaration)),
            None => declarations.extend(attribute.value_type().iter().map(declaration)),
        }

        let qualified = DeclarationTrail::below(name, Nesting::SubAttribute, owners);
        let instance_properties = attribute.instances().iter().flatten().filter_map(|instance| instance.properties().as_ref());
        for properties in attribute.properties().iter().chain(instance_properties) {
            collect(Some(&qualified), properties, declarations);
        }
    }
}

/// The refusal for a set of declarations that could not be expanded, naming the declaration whose
/// value type expands to no IRI, or the first one when the `@context` itself failed.
fn unexpandable(declarations: &[Declaration<'_>], first: &Declaration<'_>, source: ExpansionError) -> MappingError {
    let culprit = match &source {
        ExpansionError::NotAnIri { value_type } => declarations.iter().find(|declaration| declaration.value_type == value_type).unwrap_or(first),
        ExpansionError::CoreContext(_) | ExpansionError::Client(_) | ExpansionError::Context(_) | ExpansionError::Expansion(_) => first,
    };

    MappingError::UnexpandableValueType {
        attribute: culprit.path.clone(),
        value_type: Box::new(culprit.value_type.clone()),
        source: Box::new(source),
    }
}

/// Checks one declaration against its attribute's type and conversion.
fn check(declaration: &Declaration<'_>, expanded: &ExpandedValueTypes, lang_string: &ValueType) -> Result<()> {
    let Declaration { path, attribute, value_type } = declaration;
    let value_type = *value_type;

    if let NgsiLdAttributeKind::LanguageProperty = attribute.kind() {
        return if expanded.same_datatype(value_type, lang_string) {
            Ok(())
        } else {
            Err(MappingError::LanguagePropertyValueType {
                attribute: path.clone(),
                value_type: Box::new(value_type.clone()),
            })
        };
    }

    let named = TIME_DATA_TYPES
        .into_iter()
        .find(|time| expanded.same_datatype(value_type, &ValueType::from(*time)));

    match (attribute.conversion().temporal_reading(), named) {
        (Some(implied), Some(named)) if implied == named => Ok(()),
        (Some(implied), Some(_) | None) => Err(MappingError::ValueTypeContradictsTransformation {
            attribute: path.clone(),
            value_type: Box::new(value_type.clone()),
            transformation: Transformation::reading(implied),
            implied,
        }),
        (None, Some(named)) => Err(MappingError::TimeValueTypeWithoutTransformation {
            attribute: path.clone(),
            value_type: Box::new(value_type.clone()),
            named,
            needed: Transformation::reading(named),
        }),
        (None, None) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::MappingError, mapping::Mapping, template::runner::TemplateRunner, value_type_agreement::check_value_types};
    use cassiopeia_common::user_agent::UserAgent;
    use cassiopeia_ngsi_ld::{
        entity::{context::ContextSource, name::NameBuf},
        value::types::TemporalValueType,
    };
    use cassiopeia_value_type_expansion::{error::ExpansionError, value_type_expander::ValueTypeExpander};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::{collections::HashMap, path::Path};

    fn mapping(attributes: &str) -> Mapping {
        let document = format!(
            r#"{{
                version: "v4",
                dataModel: "Alert",
                identity: {{ entityName: "Alert-{{{{ id }}}}" }},
                attributes: {attributes},
            }}"#
        );

        Mapping::from_json5(&document, Path::new("test.json5"), &mut TemplateRunner::new()).unwrap()
    }

    fn checked(attributes: &str, contexts: &ContextSource) -> Result<(), MappingError> {
        let expander = ValueTypeExpander::new(&UserAgent::from("test".to_owned())).unwrap();

        check_value_types(&mapping(attributes), &expander, contexts)
    }

    fn checked_under_core(attributes: &str) -> Result<(), MappingError> {
        checked(attributes, &ContextSource::None)
    }

    #[test]
    fn a_time_value_type_matching_its_transformation_loads_whichever_way_it_is_spelt() {
        for spelling in ["DateTime", "ngsi-ld:DateTime", "https://uri.etsi.org/ngsi-ld/DateTime"] {
            let attributes = format!(r#"{{ due: {{ source: "{{{{ due }}}}", transformation: "datetime", valueType: "{spelling}" }} }}"#);

            assert!(checked_under_core(&attributes).is_ok(), "{spelling}");
        }
        assert!(checked_under_core(r#"{ day: { source: "{{ day }}", transformation: "date", valueType: "Date" } }"#).is_ok());
        assert!(checked_under_core(r#"{ at: { source: "{{ at }}", transformation: "time", valueType: "Time" } }"#).is_ok());
    }

    #[test]
    fn a_value_type_contradicting_a_time_transformation_is_refused_naming_both() {
        let error = checked_under_core(r#"{ due: { source: "{{ due }}", transformation: "datetime", valueType: "Date" } }"#)
            .expect_err("Date is not what datetime writes");

        assert!(matches!(
            &error,
            MappingError::ValueTypeContradictsTransformation { attribute, implied: TemporalValueType::DateTime, .. } if attribute.to_string() == "due"
        ));
        assert_eq!(
            error.to_string(),
            r#"Attribute `due` declares `valueType: "Date"`, but `transformation: "datetime"` produces a `DateTime`; declare `DateTime` or remove the `valueType`"#
        );
    }

    #[test]
    fn a_value_type_that_is_no_time_beside_a_time_transformation_is_refused() {
        let error =
            checked_under_core(r#"{ due: { source: "{{ due }}", transformation: "time", valueType: "PostalAddress" } }"#).expect_err("a time is written");

        assert!(matches!(
            error,
            MappingError::ValueTypeContradictsTransformation {
                implied: TemporalValueType::Time,
                ..
            }
        ));
    }

    #[test]
    fn a_time_value_type_on_a_value_not_read_as_a_time_is_refused_naming_the_transformation_to_add() {
        let error = checked_under_core(r#"{ due: { source: "{{ due }}", valueType: "ngsi-ld:DateTime" } }"#).expect_err("the text is not read as a time");

        assert!(matches!(
            &error,
            MappingError::TimeValueTypeWithoutTransformation {
                named: TemporalValueType::DateTime,
                ..
            }
        ));
        assert!(error.to_string().ends_with(r#"add `transformation: "datetime"`"#), "{error}");
    }

    #[test]
    fn a_value_type_naming_no_time_on_any_other_transformation_loads() {
        assert!(checked_under_core(r#"{ address: { source: "{{ address }}", valueType: "PostalAddress" } }"#).is_ok());
        assert!(checked_under_core(r#"{ price: { source: "{{ price }}", transformation: "float", valueType: "xsd:decimal" } }"#).is_ok());
    }

    #[test]
    fn a_language_property_value_type_expanding_to_lang_string_loads() {
        for spelling in ["langString", "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString"] {
            let attributes = format!(r#"{{ name: {{ type: "LanguageProperty", source: "{{{{ name }}}}", valueType: "{spelling}" }} }}"#);

            assert!(checked_under_core(&attributes).is_ok(), "{spelling}");
        }
    }

    #[test]
    fn a_language_property_value_type_other_than_lang_string_is_refused() {
        let error = checked_under_core(r#"{ name: { type: "LanguageProperty", source: "{{ name }}", valueType: "String" } }"#).expect_err("only langString");

        assert!(matches!(&error, MappingError::LanguagePropertyValueType { attribute, .. } if attribute.to_string() == "name"));
        assert!(error.to_string().contains("table 5.2.32-1"));
    }

    #[test]
    fn an_instance_value_type_is_checked_against_the_shared_transformation() {
        let error = checked_under_core(
            r#"{
                due: {
                    transformation: "datetime",
                    instances: [
                        { source: "{{ a }}", valueType: "DateTime" },
                        { source: "{{ b }}", valueType: "Time" },
                    ],
                },
            }"#,
        )
        .expect_err("the second instance contradicts the transformation");

        assert!(matches!(&error, MappingError::ValueTypeContradictsTransformation { value_type, .. } if value_type.as_str() == "Time"));
    }

    #[test]
    fn a_sub_attribute_value_type_is_checked_under_its_path() {
        let error = checked_under_core(r#"{ reading: { source: "{{ r }}", properties: { takenAt: { source: "{{ t }}", valueType: "Date" } } } }"#)
            .expect_err("the sub-attribute's text is not read as a date");

        assert!(matches!(&error, MappingError::TimeValueTypeWithoutTransformation { attribute, .. } if attribute.to_string() == "reading.takenAt"));
    }

    #[test]
    fn a_value_type_on_a_sub_attribute_of_an_instance_is_checked_under_its_path() {
        let error = checked_under_core(
            r#"{
                reading: {
                    instances: [{ source: "{{ r }}", properties: { takenAt: { source: "{{ t }}", valueType: "Time" } } }],
                },
            }"#,
        )
        .expect_err("the instance's sub-attribute text is not read as a time");

        assert!(matches!(&error, MappingError::TimeValueTypeWithoutTransformation { attribute, .. } if attribute.to_string() == "reading.takenAt"));
    }

    #[test]
    fn a_list_of_times_agrees_with_the_value_type_its_items_have() {
        assert!(checked_under_core(r#"{ slots: { type: "ListProperty", source: "{{ slots }}", transformation: "time", valueType: "Time" } }"#).is_ok());
    }

    #[test]
    fn a_value_type_contradicting_the_items_of_a_list_of_times_is_refused() {
        let error = checked_under_core(r#"{ slots: { type: "ListProperty", source: "{{ slots }}", transformation: "time", valueType: "DateTime" } }"#)
            .expect_err("each item is a Time, which the valueList's valueType types");

        assert!(matches!(
            error,
            MappingError::ValueTypeContradictsTransformation {
                implied: TemporalValueType::Time,
                ..
            }
        ));
    }

    #[test]
    fn a_time_value_type_on_a_list_whose_items_are_not_read_as_times_is_refused() {
        let error =
            checked_under_core(r#"{ slots: { type: "ListProperty", source: "{{ slots }}", valueType: "Time" } }"#).expect_err("the items are kept as read");

        assert!(matches!(
            error,
            MappingError::TimeValueTypeWithoutTransformation {
                named: TemporalValueType::Time,
                ..
            }
        ));
    }

    #[test]
    fn a_user_context_term_mapped_to_a_time_data_type_names_that_data_type() {
        let context = serde_json::from_value(json!({"Due": "https://uri.etsi.org/ngsi-ld/DateTime"})).unwrap();
        let contexts = ContextSource::PerEntityType(HashMap::from([(NameBuf::new("Alert").unwrap(), context)]));

        let error = checked(r#"{ due: { source: "{{ due }}", valueType: "Due" } }"#, &contexts).expect_err("Due names a DateTime under this context");

        assert!(matches!(
            error,
            MappingError::TimeValueTypeWithoutTransformation {
                named: TemporalValueType::DateTime,
                ..
            }
        ));
    }

    #[test]
    fn a_value_type_expanding_to_no_iri_is_refused_naming_its_attribute() {
        let context = serde_json::from_value(json!({"Anonymous": "_:b0"})).unwrap();

        let error =
            checked(r#"{ shape: { source: "{{ s }}", valueType: "Anonymous" } }"#, &ContextSource::Static(context)).expect_err("a blank node is no datatype");

        assert!(
            matches!(&error, MappingError::UnexpandableValueType { attribute, source, .. } if attribute.to_string() == "shape" && matches!(**source, ExpansionError::NotAnIri { .. }))
        );
    }

    #[test]
    fn a_value_type_inside_a_synthetic_entity_is_checked_too() {
        let error = checked_under_core(
            r#"{
                site: {
                    type: "Relationship",
                    source: "{{ id }}",
                    target: { entity: "Site" },
                    syntheticEntity: {
                        dataModel: "Site",
                        identity: { entityName: "Site-{{ id }}" },
                        attributes: { openedOn: { source: "{{ opened }}", transformation: "date", valueType: "DateTime" } },
                    },
                },
            }"#,
        )
        .expect_err("date writes a Date");

        assert!(matches!(&error, MappingError::ValueTypeContradictsTransformation { attribute, .. } if attribute.to_string() == "openedOn"));
    }

    #[test]
    fn a_mapping_declaring_no_value_type_is_not_expanded() {
        let contexts = ContextSource::Static(serde_json::from_value(json!("http://127.0.0.1:9/unreachable.jsonld")).unwrap());

        assert!(checked(r#"{ temperature: { source: "{{ t }}", transformation: "float" } }"#, &contexts).is_ok());
    }

    #[test]
    fn a_context_that_cannot_be_loaded_refuses_the_mapping() {
        let contexts = ContextSource::Static(serde_json::from_value(json!("http://127.0.0.1:9/unreachable.jsonld")).unwrap());

        let error = checked(r#"{ address: { source: "{{ a }}", valueType: "PostalAddress" } }"#, &contexts).expect_err("the context cannot be loaded");

        assert!(
            matches!(&error, MappingError::UnexpandableValueType { attribute, source, .. } if attribute.to_string() == "address" && matches!(**source, ExpansionError::Expansion(_)))
        );
    }
}
