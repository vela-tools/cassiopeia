use crate::{
    attribute::{
        Attribute,
        Attributes,
        declaration_trail::{DeclarationTrail, Nesting},
    },
    declaration_site::{Declaration, DeclarationSite, DeclaringEntity, InstancePosition},
    document_key::DocumentKey,
    ignored_key::{IgnoredKey, IgnoredKeyKind, IneffectiveKey},
    unread_keys::UnreadKeys,
};
use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};

/// The keys a `target` is given that nothing reads: a relationship's object is the entity its
/// `source` names, with the data model `target.entity` names.
const UNREAD_TARGET_KEYS: [IneffectiveKey; 2] = [IneffectiveKey::SourceField, IneffectiveKey::IdPattern];

/// Where a declaration sits in the entity it describes.
#[derive(Clone, Copy, Debug)]
enum Level {
    /// A top-level attribute of the entity.
    TopLevel,
    /// A declaration nested beneath an attribute: a sub-attribute, a member, a `languageMap` entry,
    /// or anything beneath them.
    Nested,
}

/// Takes out every key the declarations of `attributes`, a document's own, hold without effect, in
/// document order, and detaches each `syntheticEntity` that no entity is written from, so the loaded
/// mapping holds only what takes effect.
///
/// The expander writes a synthetic entity only from a top-level Relationship or `ListRelationship` of
/// the document, whose link to the entity the synthetic identity mints. One declared anywhere else,
/// on a Property, beneath an attribute or inside another synthetic entity, would write an entity no
/// entity links to, or no entity at all, so it is dropped whole and the keys inside it go unreported.
pub(crate) fn take(attributes: &mut Attributes) -> Vec<IgnoredKey> {
    let mut found = Vec::new();
    KeyGathering {
        entity: &DeclaringEntity::Main,
        found: &mut found,
    }
    .top_level(attributes);
    found
}

/// What a key a target holds without reading it is: one that nothing reads, or an unknown one.
fn target_key(key: DocumentKey) -> IgnoredKeyKind {
    UNREAD_TARGET_KEYS
        .into_iter()
        .find(|ineffective| key.as_str() == <&str>::from(*ineffective))
        .map_or(IgnoredKeyKind::Unknown(key), IgnoredKeyKind::WithoutEffect)
}

/// A walk over the declarations of one entity of a mapping document, gathering the keys they hold
/// without effect.
struct KeyGathering<'a> {
    /// The entity whose declarations are walked.
    entity: &'a DeclaringEntity,
    /// The ignored keys found so far, in document order.
    found: &'a mut Vec<IgnoredKey>,
}

impl KeyGathering<'_> {
    /// Walks every top-level attribute of the entity, and everything nested beneath each.
    fn top_level(&mut self, attributes: &mut Attributes) {
        for (name, attribute) in attributes {
            self.declaration(None, name, attribute, Level::TopLevel);
        }
    }

    /// Walks `attribute`, declared as `name` beneath the declarations `owners` frames, then
    /// everything nested beneath it.
    fn declaration(&mut self, owners: Option<&DeclarationTrail<'_>>, name: &NameBuf, attribute: &mut Attribute, level: Level) {
        self.unknown(attribute.unread_keys_mut(), || Declaration::Attribute(DeclarationTrail::path_of(owners, name)));
        self.beneath(owners, name, attribute, level);
    }

    /// Walks what `attribute`, declared as `name` beneath the declarations `owners` frames, holds
    /// besides its own keys: its target, its synthetic entity, its members, its `languageMap`
    /// entries, its sub-attributes and its instances.
    fn beneath(&mut self, owners: Option<&DeclarationTrail<'_>>, name: &NameBuf, attribute: &mut Attribute, level: Level) {
        let path = || DeclarationTrail::path_of(owners, name);

        if let Some(target) = attribute.target_mut() {
            for key in target.unread_keys_mut().take() {
                self.record(Declaration::Target(path()), target_key(key));
            }
        }
        self.synthetic_entity(name, attribute, level, || Declaration::Attribute(path()));

        let members = DeclarationTrail::below(name, Nesting::Member, owners);
        for (member, declaration) in attribute.mappings_mut() {
            self.declaration(Some(&members), member, declaration, Level::Nested);
        }

        // A `languageMap` entry carries no name of its own, so what is nested beneath it is named
        // under the attribute holding the entry.
        for (language, entry) in attribute.language_map_mut() {
            self.unknown(entry.unread_keys_mut(), || Declaration::LanguageEntry {
                attribute: path(),
                language: language.clone(),
            });
            self.beneath(owners, name, entry, Level::Nested);
        }

        // An instance's own `properties` are sub-attributes of the attribute, exactly as the
        // attribute-level ones are, so both are walked beneath the same frame.
        let sub_attributes = DeclarationTrail::below(name, Nesting::SubAttribute, owners);
        for (property, declaration) in attribute.properties_mut().iter_mut().flatten() {
            self.declaration(Some(&sub_attributes), property, declaration, Level::Nested);
        }
        for (index, instance) in attribute.instances_mut().iter_mut().flatten().enumerate() {
            self.unknown(instance.unread_keys_mut(), || Declaration::Instance {
                attribute: path(),
                position: InstancePosition::of_index(index),
            });
            for (property, declaration) in instance.properties_mut().iter_mut().flatten() {
                self.declaration(Some(&sub_attributes), property, declaration, Level::Nested);
            }
        }
    }

    /// Walks the `syntheticEntity` of `attribute`, declared as `name` at `level`, when an entity is
    /// written from it, and otherwise detaches it as a key without effect held by `site`.
    fn synthetic_entity(&mut self, name: &NameBuf, attribute: &mut Attribute, level: Level, site: impl FnOnce() -> Declaration) {
        let writes_entity = matches!(
            (self.entity, level, attribute.kind()),
            (
                DeclaringEntity::Main,
                Level::TopLevel,
                NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship
            )
        );
        if writes_entity {
            if let Some(synthetic) = attribute.synthetic_entity_mut() {
                KeyGathering {
                    entity: &DeclaringEntity::Synthetic(name.clone()),
                    found: &mut *self.found,
                }
                .top_level(synthetic.attributes_mut());
            }
        } else if attribute.take_synthetic_entity().is_some() {
            self.record(site(), IgnoredKeyKind::WithoutEffect(IneffectiveKey::SyntheticEntity));
        }
    }

    /// Records each of `keys` as unknown to the declaration `declaration` names.
    fn unknown(&mut self, keys: &mut UnreadKeys, declaration: impl Fn() -> Declaration) {
        for key in keys.take() {
            self.record(declaration(), IgnoredKeyKind::Unknown(key));
        }
    }

    /// Records the key `kind` names, held by `declaration` of the entity being walked.
    fn record(&mut self, declaration: Declaration, kind: IgnoredKeyKind) {
        // Each ignored key owns its site, since it outlives the walk that found it.
        let site = DeclarationSite::new(self.entity.clone(), declaration);
        self.found.push(IgnoredKey::new(site, kind));
    }
}

#[cfg(test)]
mod tests {
    use crate::{ignored_key::IgnoredKeyKind, mapping::Mapping, template::runner::TemplateRunner};
    use pretty_assertions::assert_eq;
    use std::path::Path;

    /// A `Device` mapping declaring `attributes`, loaded.
    fn device(attributes: &str) -> Mapping {
        let document = format!(r#"{{ version: "v4", dataModel: "Device", identity: {{ entityName: "{{{{ id }}}}" }}, attributes: {attributes} }}"#);
        Mapping::from_json5(&document, Path::new("device.json5"), &mut TemplateRunner::new()).expect("the mapping loads")
    }

    /// Each key loading `mapping` ignored: the declaration holding it, and the key with why it is
    /// ignored.
    fn ignored(mapping: &Mapping) -> Vec<(String, String)> {
        mapping
            .ignored_keys()
            .iter()
            .map(|ignored| {
                let key = match ignored.kind() {
                    IgnoredKeyKind::Unknown(key) => format!("unknown `{key}`"),
                    IgnoredKeyKind::WithoutEffect(key) => format!("without effect `{key}`"),
                };
                (ignored.site().to_string(), key)
            })
            .collect()
    }

    fn found(site: &str, key: &str) -> (String, String) {
        (site.to_owned(), key.to_owned())
    }

    #[test]
    fn an_attribute_holding_a_key_it_does_not_read_loads_and_names_the_attribute_and_the_key() {
        let mapping = device(r#"{ temperature: { source: "{{ t }}", transformaton: "float" } }"#);

        assert_eq!(ignored(&mapping), [found("`temperature`", "unknown `transformaton`")]);
        assert_eq!(mapping.attributes()[0].transformation(), &None);
    }

    #[test]
    fn a_sub_attribute_holding_a_key_it_does_not_read_is_named_by_its_path() {
        let mapping = device(r#"{ temperature: { source: "{{ t }}", properties: { observedAt: { source: "{{ ts }}", trasformation: "datetime" } } } }"#);

        assert_eq!(ignored(&mapping), [found("`temperature.observedAt`", "unknown `trasformation`")]);
    }

    #[test]
    fn a_member_holding_a_key_it_does_not_read_is_named_by_its_path() {
        let mapping = device(r#"{ address: { mappings: { street: { source: "{{ street }}", trasformation: "string" } } } }"#);

        assert_eq!(ignored(&mapping), [found("`address[street]`", "unknown `trasformation`")]);
    }

    #[test]
    fn a_language_map_entry_holding_a_key_it_does_not_read_is_named_by_its_attribute_and_language_tag() {
        let mapping = device(r#"{ label: { type: "LanguageProperty", languageMap: { en: { source: "{{ en }}", sorce: "{{ name }}" } } } }"#);

        assert_eq!(ignored(&mapping), [found("The languageMap entry `en` of `label`", "unknown `sorce`")]);
    }

    #[test]
    fn an_instance_holding_a_key_it_does_not_read_is_named_by_its_attribute_and_its_position_from_one() {
        let mapping = device(
            r#"{
                forecast: {
                    transformation: "float",
                    instances: [
                        { source: "{{ a }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:a" } } },
                        { source: "{{ b }}", unitCode: "CEL", properties: { datasetId: { source: "urn:ngsi-ld:dataset:b" } } },
                    ],
                },
            }"#,
        );

        assert_eq!(ignored(&mapping), [found("Instance 2 of `forecast`", "unknown `unitCode`")]);
    }

    #[test]
    fn a_target_holding_a_key_it_does_not_read_is_named_by_its_relationship() {
        let mapping = device(r#"{ refBuilding: { type: "Relationship", source: "{{ b }}", target: { entity: "Building", entiy: "Site" } } }"#);

        assert_eq!(ignored(&mapping), [found("The target of `refBuilding`", "unknown `entiy`")]);
        assert_eq!(
            mapping.attributes()[0].target().as_ref().map(|target| target.entity().entity_type().as_str()),
            Some("Building")
        );
    }

    #[test]
    fn a_target_s_source_field_and_id_pattern_are_keys_without_effect() {
        let mapping = device(
            r#"{ refBuilding: { type: "Relationship", source: "{{ b }}", target: { entity: "Building", sourceField: "building_uri", idPattern: "B-\\d+" } } }"#,
        );

        assert_eq!(
            ignored(&mapping),
            [
                found("The target of `refBuilding`", "without effect `sourceField`"),
                found("The target of `refBuilding`", "without effect `idPattern`"),
            ]
        );
    }

    #[test]
    fn a_synthetic_entity_on_a_property_is_a_key_without_effect_and_is_dropped_whole() {
        let mapping = device(
            r#"{
                manufacturer: {
                    source: "{{ maker }}",
                    syntheticEntity: {
                        dataModel: "Organization",
                        identity: { entityName: "{{ maker }}" },
                        attributes: { name: { source: "{{ maker }}", sorce: "{{ maker }}" } },
                    },
                },
            }"#,
        );

        assert_eq!(ignored(&mapping), [found("`manufacturer`", "without effect `syntheticEntity`")]);
        assert!(mapping.attributes()[0].synthetic_entity().is_none());
    }

    #[test]
    fn a_synthetic_entity_on_a_relationship_or_a_list_relationship_is_kept_without_a_warning() {
        let mapping = device(
            r#"{
                owner: {
                    type: "Relationship",
                    syntheticEntity: { dataModel: "Organization", identity: { entityName: "{{ owner }}" }, attributes: {} },
                },
                hasCountry: {
                    type: "ListRelationship",
                    source: "{{ countries }}",
                    target: { entity: "Country" },
                    syntheticEntity: { dataModel: "Country", identity: { entityName: "{{ this[0] }}" }, attributes: {} },
                },
            }"#,
        );

        assert_eq!(ignored(&mapping), []);
        assert!(mapping.attributes().values().all(|attribute| attribute.synthetic_entity().is_some()));
    }

    #[test]
    fn a_synthetic_entity_on_a_nested_relationship_is_a_key_without_effect() {
        let mapping = device(
            r#"{
                hasLeadActor: {
                    type: "Relationship",
                    source: "{{ actor }}",
                    target: { entity: "Person" },
                    properties: {
                        playsCharacter: {
                            type: "Relationship",
                            source: "{{ character }}",
                            target: { entity: "Character" },
                            syntheticEntity: { dataModel: "Character", identity: { entityName: "{{ character }}" }, attributes: {} },
                        },
                    },
                },
            }"#,
        );

        assert_eq!(ignored(&mapping), [found("`hasLeadActor.playsCharacter`", "without effect `syntheticEntity`")]);
    }

    #[test]
    fn a_key_inside_a_synthetic_entity_is_named_by_the_attribute_declaring_the_synthetic_entity() {
        let mapping = device(
            r#"{
                owner: {
                    type: "Relationship",
                    syntheticEntity: {
                        dataModel: "Organization",
                        identity: { entityName: "{{ owner }}" },
                        attributes: {
                            parent: {
                                type: "Relationship",
                                source: "{{ parent }}",
                                target: { entity: "Organization" },
                                syntheticEntity: { dataModel: "Organization", identity: { entityName: "{{ parent }}" }, attributes: {} },
                            },
                            name: { source: "{{ owner_name }}", sorce: "{{ owner }}" },
                        },
                    },
                },
            }"#,
        );

        assert_eq!(
            ignored(&mapping),
            [
                found("`parent` in the syntheticEntity of `owner`", "without effect `syntheticEntity`"),
                found("`name` in the syntheticEntity of `owner`", "unknown `sorce`"),
            ]
        );
    }

    #[test]
    fn names_and_language_tags_that_spell_keys_are_declarations_and_warn_about_nothing() {
        let mapping = device(
            r#"{
                sourceField: { source: "{{ a }}" },
                transformaton: { source: "{{ b }}", properties: { idPattern: { source: "{{ c }}" }, instances: { source: "{{ d }}" } } },
                address: { mappings: { languageMap: { source: "{{ e }}" }, target: { source: "{{ f }}" } } },
                label: { type: "LanguageProperty", languageMap: { "pt-BR": { source: "{{ g }}" }, "sr-Latn-RS": { source: "{{ h }}" } } },
            }"#,
        );

        assert_eq!(ignored(&mapping), []);
        assert_eq!(mapping.iter_all_attributes().count(), 10);
    }
}
