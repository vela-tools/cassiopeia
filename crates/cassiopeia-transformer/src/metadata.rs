use crate::{attribute_builder::build_sub_attribute, qualifier_cache::QualifierCache};
use cassiopeia_ir::{
    metadata::EntityMetadata,
    sub_attribute::{SubAttribute, SubAttributes},
};
use cassiopeia_ngsi_ld::{
    entity::{attribute::NestedAttributes, name::NameBuf},
    value::types::Value,
};
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};
use serde_json::Value as JsonValue;
use urn_rs::Urn;

/// The NGSI-LD metadata resolved for one attribute (or one relationship instance).
///
/// The three standard qualifiers NGSI-LD defines on an attribute (ETSI GS CIM 009 v1.9.1 clause
/// 4.5.2.2) are lifted into typed fields; anything else the mapping declared as an attribute property
/// is carried as a nested Property in `custom_attributes`.
pub struct MetadataSummary {
    /// The `observedAt` temporal qualifier.
    pub observed_at: Option<DateTime<Utc>>,
    /// The `unitCode` qualifier, valid only on a Property.
    pub unit_code: Option<UnitCode>,
    /// The `datasetId` qualifier (NGSI-LD 4.5.5: a URI).
    pub dataset_id: Option<Urn>,
    /// Any further declared sub-attributes, each built as the NGSI-LD kind it was declared as (a
    /// Property or one of its subclasses, ETSI GS CIM 009 v1.9.1 clause 4.5.2.2).
    pub custom_attributes: NestedAttributes,
}

/// Resolves the metadata declared for one attribute into typed NGSI-LD qualifiers.
///
/// `index` selects one entry of per-item metadata (used for the instances of a `ListRelationship`);
/// when it is `None`, the attribute's shared metadata is read.
///
/// `cache` carries the already-parsed `observedAt` texts and unit codes a mapping repeats across
/// attributes, and `unreadable` gathers the `observedAt` texts that would not read at all.
#[must_use]
pub fn transform_metadata(
    metadata: Option<&EntityMetadata>,
    attr_name: &NameBuf,
    index: Option<usize>,
    cache: &mut QualifierCache<'_>,
    unreadable: &UnreadableTimestamps,
) -> MetadataSummary {
    let properties = metadata.and_then(|metadata| metadata.get(attr_name.as_str())).and_then(|storage| match index {
        Some(index) => storage.get_for_index(index),
        None => storage.as_shared(),
    });

    match properties {
        Some(properties) => summary_from(properties, attr_name, cache, unreadable),
        None => empty_summary(),
    }
}

/// Records an `observedAt` that would not read as an instant.
///
/// An attribute whose `observedAt` is dropped is still published, so nothing downstream can tell the
/// qualifier was ever meant to be there: a temporal series built from it has no instant to fold on
/// (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2). Only text is recorded, and only when it is not blank: a
/// blank qualifier is one the record does not carry, and a non-text one is a mapping mistake with no
/// spelling to quote back.
fn record_unreadable_observed_at(attribute: &NameBuf, value: &JsonValue, unreadable: &UnreadableTimestamps) {
    if let Some(text) = value.as_str().map(str::trim).filter(|text| !text.is_empty()) {
        unreadable.record(attribute, text);
    }
}

/// An empty summary: no qualifiers and no sub-attributes.
fn empty_summary() -> MetadataSummary {
    MetadataSummary {
        observed_at: None,
        unit_code: None,
        dataset_id: None,
        custom_attributes: NestedAttributes::default(),
    }
}

/// Folds a sub-attribute map into a [`MetadataSummary`], recursively.
///
/// The three standard qualifiers (NGSI-LD 4.5.2.2) are lifted by name into typed fields; every other
/// entry becomes a sub-attribute built as the NGSI-LD kind it declared, carrying its own qualifiers
/// and, recursively, its own sub-attributes (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2). A sub-attribute
/// whose builder rejects its value is dropped rather than emitted malformed.
///
/// `attribute` names the attribute these properties qualify, so an `observedAt` that will not read
/// is recorded against it, including one declared on a nested sub-attribute, which is a timestamp
/// that same attribute lost.
fn summary_from(props: &SubAttributes, attribute: &NameBuf, cache: &mut QualifierCache<'_>, unreadable: &UnreadableTimestamps) -> MetadataSummary {
    let mut summary = empty_summary();

    if let Some(observed_at) = props.get("observedAt").map(SubAttribute::value) {
        match cache.observed_at(observed_at) {
            Some(datetime) => summary.observed_at = Some(datetime),
            None => record_unreadable_observed_at(attribute, observed_at, unreadable),
        }
    }
    if let Some(unit_code) = props.get("unitCode").map(SubAttribute::value).and_then(JsonValue::as_str) {
        summary.unit_code = cache.unit_code(unit_code);
    }
    // An invalid `datasetId` is dropped rather than emitted malformed: the qualifier is optional
    // (NGSI-LD 4.5.5) and must be a URI.
    if let Some(dataset_id) = props.get("datasetId").map(SubAttribute::value).and_then(JsonValue::as_str) {
        summary.dataset_id = dataset_id.parse().ok();
    }

    for (name, sub) in props {
        if matches!(name.as_str(), "observedAt" | "unitCode" | "datasetId") {
            continue;
        }
        let sub_meta = summary_from(sub.metadata(), attribute, cache, unreadable);
        if let Some(wrapper) = build_sub_attribute(*sub.kind(), Value::from(sub.value().clone()), sub.object_type().clone(), sub_meta) {
            summary.custom_attributes.insert(name.clone(), Box::new(wrapper));
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use crate::{
        metadata::{MetadataSummary, transform_metadata},
        observed_at_cache::ObservedAtCache,
        qualifier_cache::QualifierCache,
        unit_code_cache::UnitCodeCache,
    };
    use cassiopeia_ir::{metadata::MetadataStorage, sub_attribute::SubAttribute};
    use cassiopeia_ngsi_ld::entity::{
        attribute::{NgsiLdAttribute, NgsiLdAttributeKind, NgsiLdAttributeWrapper},
        name::NameBuf,
    };
    use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;
    use indexmap::IndexMap;
    use serde_json::{Value, json};

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).unwrap()
    }

    fn sub(kind: NgsiLdAttributeKind, value: Value) -> SubAttribute {
        SubAttribute::new(kind, value, IndexMap::default())
    }

    fn attribute(summary: &MetadataSummary, key: &str) -> NgsiLdAttribute {
        let NgsiLdAttributeWrapper::Single(attr) = summary.custom_attributes.get(&name(key)).expect("sub-attribute present").as_ref() else {
            panic!("expected a single sub-attribute");
        };
        attr.as_ref().clone()
    }

    #[test]
    fn absent_metadata_yields_an_empty_summary() {
        let summary = transform_metadata(
            None,
            &name("temperature"),
            None,
            &mut QualifierCache::new(&mut ObservedAtCache::new(), &mut UnitCodeCache::new()),
            &UnreadableTimestamps::new(),
        );

        assert!(summary.observed_at.is_none());
        assert!(summary.unit_code.is_none());
        assert!(summary.dataset_id.is_none());
        assert!(summary.custom_attributes.is_empty());
    }

    #[test]
    fn the_standard_qualifiers_are_lifted_and_the_rest_become_custom_attributes() {
        let mut shared = IndexMap::default();
        shared.insert(name("observedAt"), sub(NgsiLdAttributeKind::Property, json!("2026-04-03T22:00:20Z")));
        shared.insert(name("unitCode"), sub(NgsiLdAttributeKind::Property, json!("CEL")));
        shared.insert(name("accuracy"), sub(NgsiLdAttributeKind::Property, json!(0.5)));
        let mut metadata = IndexMap::default();
        metadata.insert(name("temperature"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("temperature"),
            None,
            &mut QualifierCache::new(&mut ObservedAtCache::new(), &mut UnitCodeCache::new()),
            &UnreadableTimestamps::new(),
        );

        assert!(summary.observed_at.is_some());
        assert!(summary.unit_code.is_some());
        assert_eq!(summary.custom_attributes.len(), 1);
        assert!(matches!(attribute(&summary, "accuracy"), NgsiLdAttribute::Property(_)));
    }

    #[test]
    fn one_cache_shared_across_attributes_yields_the_same_summaries_as_fresh_ones() {
        let mut shared = IndexMap::default();
        shared.insert(name("observedAt"), sub(NgsiLdAttributeKind::Property, json!("2026-04-03T22:00:20Z")));
        shared.insert(name("unitCode"), sub(NgsiLdAttributeKind::Property, json!("KWH")));
        let mut metadata = IndexMap::default();
        metadata.insert(name("import"), MetadataStorage::shared(shared.clone()));
        metadata.insert(name("export"), MetadataStorage::shared(shared));

        let mut observed_at = ObservedAtCache::new();
        let mut unit_codes = UnitCodeCache::new();
        let mut cache = QualifierCache::new(&mut observed_at, &mut unit_codes);
        let first = transform_metadata(Some(&metadata), &name("import"), None, &mut cache, &UnreadableTimestamps::new());
        let second = transform_metadata(Some(&metadata), &name("export"), None, &mut cache, &UnreadableTimestamps::new());
        let fresh = transform_metadata(
            Some(&metadata),
            &name("export"),
            None,
            &mut QualifierCache::new(&mut ObservedAtCache::new(), &mut UnitCodeCache::new()),
            &UnreadableTimestamps::new(),
        );

        assert_eq!(first.observed_at, second.observed_at);
        assert_eq!(first.unit_code, second.unit_code);
        assert_eq!(second.observed_at, fresh.observed_at);
        assert_eq!(second.unit_code, fresh.unit_code);
    }

    #[test]
    fn attributes_carrying_different_qualifiers_each_resolve_through_one_cache() {
        let mut early = IndexMap::default();
        early.insert(name("observedAt"), sub(NgsiLdAttributeKind::Property, json!("2026-04-03T22:00:20Z")));
        early.insert(name("unitCode"), sub(NgsiLdAttributeKind::Property, json!("KWH")));
        let mut late = IndexMap::default();
        late.insert(name("observedAt"), sub(NgsiLdAttributeKind::Property, json!("2026-04-03T23:15:00Z")));
        late.insert(name("unitCode"), sub(NgsiLdAttributeKind::Property, json!("VLT")));
        let mut metadata = IndexMap::default();
        metadata.insert(name("energy"), MetadataStorage::shared(early));
        metadata.insert(name("voltage"), MetadataStorage::shared(late));

        let mut observed_at = ObservedAtCache::new();
        let mut unit_codes = UnitCodeCache::new();
        let mut cache = QualifierCache::new(&mut observed_at, &mut unit_codes);
        let energy = transform_metadata(Some(&metadata), &name("energy"), None, &mut cache, &UnreadableTimestamps::new());
        let voltage = transform_metadata(Some(&metadata), &name("voltage"), None, &mut cache, &UnreadableTimestamps::new());

        assert!(energy.observed_at.is_some());
        assert!(voltage.observed_at.is_some());
        assert_ne!(energy.observed_at, voltage.observed_at);
        assert_ne!(energy.unit_code, voltage.unit_code);
        assert_eq!(
            energy.unit_code,
            transform_metadata(
                Some(&metadata),
                &name("energy"),
                None,
                &mut QualifierCache::new(&mut ObservedAtCache::new(), &mut UnitCodeCache::new()),
                &UnreadableTimestamps::new(),
            )
            .unit_code
        );
    }

    #[test]
    fn a_vocab_property_sub_attribute_keeps_its_kind() {
        let mut shared = IndexMap::default();
        shared.insert(name("level"), sub(NgsiLdAttributeKind::VocabProperty, json!("https://example.org/level/high")));
        let mut metadata = IndexMap::default();
        metadata.insert(name("sugars"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("sugars"),
            None,
            &mut QualifierCache::new(&mut ObservedAtCache::new(), &mut UnitCodeCache::new()),
            &UnreadableTimestamps::new(),
        );

        let NgsiLdAttribute::VocabProperty(property) = attribute(&summary, "level") else {
            panic!("expected a vocab property sub-attribute");
        };
        assert_eq!(property.has_vocab.to_string(), "https://example.org/level/high");
    }

    #[test]
    fn a_list_property_and_a_language_property_sub_attribute_keep_their_kinds() {
        let mut shared = IndexMap::default();
        shared.insert(name("tags"), sub(NgsiLdAttributeKind::ListProperty, json!(["a", "b"])));
        shared.insert(
            name("label"),
            sub(NgsiLdAttributeKind::LanguageProperty, json!({ "en": "high", "fr": "eleve" })),
        );
        let mut metadata = IndexMap::default();
        metadata.insert(name("sugars"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("sugars"),
            None,
            &mut QualifierCache::new(&mut ObservedAtCache::new(), &mut UnitCodeCache::new()),
            &UnreadableTimestamps::new(),
        );

        assert!(matches!(attribute(&summary, "tags"), NgsiLdAttribute::ListProperty(_)));
        assert!(matches!(attribute(&summary, "label"), NgsiLdAttribute::LanguageProperty(_)));
    }

    #[test]
    fn an_untyped_sub_attribute_stays_a_property() {
        let mut shared = IndexMap::default();
        shared.insert(name("accuracy"), sub(NgsiLdAttributeKind::Property, json!(0.5)));
        let mut metadata = IndexMap::default();
        metadata.insert(name("temperature"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("temperature"),
            None,
            &mut QualifierCache::new(&mut ObservedAtCache::new(), &mut UnitCodeCache::new()),
            &UnreadableTimestamps::new(),
        );

        assert!(matches!(attribute(&summary, "accuracy"), NgsiLdAttribute::Property(_)));
    }

    #[test]
    fn a_relationship_sub_attribute_becomes_a_relationship_with_object_type_and_nested_sub_attribute() {
        let mut inner = IndexMap::default();
        inner.insert(name("billingOrder"), sub(NgsiLdAttributeKind::Property, json!(0)));
        let relationship = SubAttribute::new_relationship(
            NgsiLdAttributeKind::Relationship,
            json!("urn:ngsi-ld:Character:JackSparrow"),
            Some(name("Character")),
            inner,
        );
        let mut shared = IndexMap::default();
        shared.insert(name("playsCharacter"), relationship);
        let mut metadata = IndexMap::default();
        metadata.insert(name("hasLeadActor"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("hasLeadActor"),
            None,
            &mut QualifierCache::new(&mut ObservedAtCache::new(), &mut UnitCodeCache::new()),
            &UnreadableTimestamps::new(),
        );

        let NgsiLdAttribute::Relationship(relationship) = attribute(&summary, "playsCharacter") else {
            panic!("expected a relationship sub-attribute");
        };
        assert_eq!(relationship.object.to_string(), "urn:ngsi-ld:Character:JackSparrow");
        assert_eq!(relationship.object_type, Some(name("Character")));
        assert!(relationship.attributes.contains_key(&name("billingOrder")));
    }

    #[test]
    fn a_two_level_nested_sub_attribute_carries_its_own_sub_attribute() {
        let mut inner = IndexMap::default();
        inner.insert(name("source"), sub(NgsiLdAttributeKind::Property, json!("measured")));
        let mut shared = IndexMap::default();
        shared.insert(
            name("level"),
            SubAttribute::new(NgsiLdAttributeKind::VocabProperty, json!("https://example.org/level/high"), inner),
        );
        let mut metadata = IndexMap::default();
        metadata.insert(name("sugars"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("sugars"),
            None,
            &mut QualifierCache::new(&mut ObservedAtCache::new(), &mut UnitCodeCache::new()),
            &UnreadableTimestamps::new(),
        );

        let NgsiLdAttribute::VocabProperty(property) = attribute(&summary, "level") else {
            panic!("expected a vocab property sub-attribute");
        };
        let NgsiLdAttributeWrapper::Single(nested) = property.attributes.get(&name("source")).expect("nested sub-attribute").as_ref() else {
            panic!("expected a single nested sub-attribute");
        };
        assert!(matches!(nested.as_ref(), NgsiLdAttribute::Property(_)));
    }
}
