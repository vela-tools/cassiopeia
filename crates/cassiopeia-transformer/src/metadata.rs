use crate::{attribute_builder::build_sub_attribute, qualifier_cache::QualifierCache, qualifier_losses::QualifierLosses, unknown_unit_codes::UnknownUnitCodes};
use cassiopeia_ir::{
    metadata::EntityMetadata,
    sub_attribute::{SubAttribute, SubAttributes},
};
use cassiopeia_ngsi_ld::{
    entity::{attribute::NestedAttributes, name::NameBuf, value_type::ValueType},
    value::types::Value,
};
use cassiopeia_unreadable_timestamps::{timestamp_loss::TimestampLoss, unreadable_timestamps::UnreadableTimestamps};
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};
use urn_rs::Urn;

/// The NGSI-LD metadata resolved for one attribute (or one relationship instance).
///
/// The standard qualifiers NGSI-LD defines on an attribute (ETSI GS CIM 009 v1.9.1 clauses 4.5.2.2
/// and 4.22) are lifted into typed fields; anything else the mapping declared as an attribute
/// property is carried as a nested Property in `custom_attributes`.
pub struct MetadataSummary {
    /// The `observedAt` temporal qualifier.
    pub observed_at: Option<DateTime<Utc>>,
    /// The `expiresAt` temporal qualifier, the instant after which the value ceases to hold (ETSI
    /// GS CIM 009 v1.9.1 clause 4.22).
    pub expires_at: Option<DateTime<Utc>>,
    /// The `unitCode` qualifier, valid only on a Property.
    pub unit_code: Option<UnitCode>,
    /// The `datasetId` qualifier (NGSI-LD 4.5.5: a URI).
    pub dataset_id: Option<Urn>,
    /// Any further declared sub-attributes, each built as the NGSI-LD kind it was declared as (a
    /// Property or one of its subclasses, ETSI GS CIM 009 v1.9.1 clause 4.5.2.2).
    pub custom_attributes: NestedAttributes,
    /// The `valueType` the declaration named for the value (ETSI GS CIM 009 v1.9.1 clause 4.5.2),
    /// valid only on a Property or one of its subclasses.
    pub value_type: Option<ValueType>,
}

impl MetadataSummary {
    /// Carries the `valueType` the attribute's declaration named, which lives on the declaration
    /// rather than among the properties this summary is resolved from.
    #[must_use]
    pub fn with_value_type(self, value_type: Option<ValueType>) -> MetadataSummary {
        MetadataSummary { value_type, ..self }
    }
}

/// Resolves the metadata declared for one attribute into typed NGSI-LD qualifiers.
///
/// `index` selects one entry of per-item metadata (used for the instances of a `ListRelationship`);
/// when it is `None`, the attribute's shared metadata is read.
///
/// `cache` carries the already-parsed `observedAt` texts and unit codes a mapping repeats across
/// attributes, and `losses` gathers the qualifiers that had to be left off: an `observedAt` or
/// `expiresAt` that would not read at all, and a `unitCode` that is no UN/CEFACT code.
#[must_use]
pub fn transform_metadata(
    metadata: Option<&EntityMetadata>,
    attr_name: &NameBuf,
    index: Option<usize>,
    cache: &mut QualifierCache<'_>,
    losses: &QualifierLosses,
) -> MetadataSummary {
    let properties = metadata.and_then(|metadata| metadata.get(attr_name.as_str())).and_then(|storage| match index {
        Some(index) => storage.get_for_index(index),
        None => storage.as_shared(),
    });

    match properties {
        Some(properties) => summary_from(properties, attr_name, cache, losses),
        None => empty_summary(),
    }
}

/// Records a temporal qualifier that would not read as an instant.
///
/// An attribute whose qualifier is dropped is still published, so nothing downstream can tell the
/// qualifier was ever meant to be there: a temporal series built from `observedAt` has no instant to
/// fold on (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2), and an `expiresAt` that would have bounded the
/// value's validity is gone (clause 4.22). Which values are "not carried" rather than unreadable is
/// the shared rule in
/// [`Value::reported_text`].
fn record_unreadable(loss: TimestampLoss, attribute: &NameBuf, value: &Value, unreadable: &UnreadableTimestamps) {
    if let Some(text) = value.reported_text() {
        unreadable.record(loss, attribute, text.as_ref());
    }
}

/// The UN/CEFACT code a `unitCode` qualifier carries, recording the qualifier as lost when it carries
/// none.
///
/// ETSI GS CIM 009 v1.9.1 clause 4.5.2 admits only a Recommendation No. 20 common code as a
/// `unitCode`, so any other text leaves the attribute without one; the attribute is still published,
/// and the loss is recorded against `attribute` so the run can name the text. The code is looked up
/// as the run would quote it, by the rule that tells a value "not carried" from one that is present
/// ([`Value::reported_text`]): text without the whitespace around it, and any other value by its JSON
/// rendering, so a source that holds the code `59` as a number still yields it.
fn unit_code(value: &Value, attribute: &NameBuf, cache: &mut QualifierCache<'_>, unknown: &UnknownUnitCodes) -> Option<UnitCode> {
    let text = value.reported_text()?;
    let code = cache.unit_code(&text);
    if code.is_none() {
        unknown.record(attribute, &text);
    }
    code
}

/// The NGSI-LD Value a qualifier sub-attribute carries; a qualifier declared as raw JSON carries
/// none.
fn qualifier(sub: &SubAttribute) -> Option<&Value> {
    sub.value().as_value()
}

/// An empty summary: no qualifiers and no sub-attributes.
fn empty_summary() -> MetadataSummary {
    MetadataSummary {
        observed_at: None,
        expires_at: None,
        unit_code: None,
        dataset_id: None,
        custom_attributes: NestedAttributes::default(),
        value_type: None,
    }
}

/// Folds a sub-attribute map into a [`MetadataSummary`], recursively.
///
/// The standard qualifiers (NGSI-LD 4.5.2.2 with 4.22) are lifted by name into typed fields; every
/// other entry becomes a sub-attribute built as the NGSI-LD kind it declared, carrying its own
/// qualifiers and, recursively, its own sub-attributes (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2). A
/// sub-attribute whose builder rejects its value is dropped rather than emitted malformed.
///
/// `attribute` names the attribute these properties qualify, so an `observedAt` or `expiresAt` that
/// will not read, or a `unitCode` that is no code, is recorded against it, including one declared on
/// a nested sub-attribute, which is a qualifier that same attribute lost.
fn summary_from(props: &SubAttributes, attribute: &NameBuf, cache: &mut QualifierCache<'_>, losses: &QualifierLosses) -> MetadataSummary {
    let mut summary = empty_summary();

    if let Some(observed_at) = props.get("observedAt").and_then(qualifier) {
        match cache.instant(observed_at) {
            Some(datetime) => summary.observed_at = Some(datetime),
            None => record_unreadable(TimestampLoss::ObservedAt, attribute, observed_at, &losses.timestamps),
        }
    }
    if let Some(expires_at) = props.get("expiresAt").and_then(qualifier) {
        match cache.instant(expires_at) {
            Some(datetime) => summary.expires_at = Some(datetime),
            None => record_unreadable(TimestampLoss::ExpiresAt, attribute, expires_at, &losses.timestamps),
        }
    }
    if let Some(declared) = props.get("unitCode").and_then(qualifier) {
        summary.unit_code = unit_code(declared, attribute, cache, &losses.unit_codes);
    }
    // An invalid `datasetId` is dropped rather than emitted malformed: the qualifier is optional
    // (NGSI-LD 4.5.5) and must be a URI.
    if let Some(dataset_id) = props.get("datasetId").and_then(qualifier).and_then(Value::as_str) {
        summary.dataset_id = dataset_id.parse().ok();
    }

    for (name, sub) in props {
        if matches!(name.as_str(), "observedAt" | "expiresAt" | "unitCode" | "datasetId") {
            continue;
        }
        // Shared metadata answers for every instance, so each sub-attribute takes its own copy of the
        // datatype its declaration named.
        let sub_meta = summary_from(sub.metadata(), attribute, cache, losses).with_value_type(sub.value_type().clone());
        // The metadata is borrowed rather than consumed because shared metadata answers for every
        // instance of a multi-instance attribute, so each sub-attribute built from it takes a copy.
        if let Some(wrapper) = build_sub_attribute(*sub.kind(), sub.value().clone(), sub.object_type().clone(), sub_meta) {
            summary.custom_attributes.insert(name.clone(), Box::new(wrapper));
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use crate::{
        instant_cache::InstantCache,
        metadata::{MetadataSummary, transform_metadata},
        qualifier_cache::QualifierCache,
        qualifier_losses::QualifierLosses,
        unit_code_cache::UnitCodeCache,
    };
    use cassiopeia_batch_sink::{batch_sink::BatchSink, recurring::Recurring};
    use cassiopeia_geometry::geometry::NgsiLdGeometry;
    use cassiopeia_ir::{attribute_content::AttributeContent, metadata::MetadataStorage, sub_attribute::SubAttribute};
    use cassiopeia_ngsi_ld::{
        entity::{
            attribute::{NgsiLdAttribute, NgsiLdAttributeKind, NgsiLdAttributeWrapper},
            name::NameBuf,
            value_type::ValueType,
        },
        value::types::{TemporalValue, Value},
    };
    use cassiopeia_unreadable_timestamps::{attribute_loss::AttributeLoss, timestamp_loss::TimestampLoss};
    use cefact_units::UnitCode;
    use chrono::{TimeZone, Utc};
    use indexmap::IndexMap;
    use iri_rs::IriBuf;
    use langtag::LangTagBuf;
    use pretty_assertions::{assert_eq, assert_ne};
    use serde_json::{Value as JsonValue, json};

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).unwrap()
    }

    fn sub(kind: NgsiLdAttributeKind, value: JsonValue) -> SubAttribute {
        SubAttribute::new(kind, AttributeContent::Value(Value::try_from(value).unwrap()), IndexMap::default())
    }

    /// The vocabulary IRI content a `VocabProperty` sub-attribute resolves to.
    fn vocab(iri: &str) -> AttributeContent {
        AttributeContent::Vocab(IriBuf::new(iri.to_owned()).unwrap())
    }

    fn attribute(summary: &MetadataSummary, key: &str) -> NgsiLdAttribute {
        let NgsiLdAttributeWrapper::Single(attr) = summary.custom_attributes.get(&name(key)).expect("sub-attribute present").as_ref() else {
            panic!("expected a single sub-attribute");
        };
        attr.as_ref().clone()
    }

    #[test]
    fn a_sub_attribute_carries_the_value_type_its_declaration_named() {
        let declared = ValueType::new("PostalAddress").unwrap();
        let mut shared = IndexMap::default();
        shared.insert(
            name("formatted"),
            sub(NgsiLdAttributeKind::Property, json!("Pariser Platz")).with_value_type(Some(declared.clone())),
        );
        let mut metadata = IndexMap::default();
        metadata.insert(name("address"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("address"),
            None,
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            &QualifierLosses::new(),
        );

        let NgsiLdAttribute::Property(formatted) = attribute(&summary, "formatted") else {
            panic!("expected a property sub-attribute");
        };
        assert_eq!(formatted.value_type, Some(declared));
        assert_eq!(summary.value_type, None);
    }

    #[test]
    fn absent_metadata_yields_an_empty_summary() {
        let summary = transform_metadata(
            None,
            &name("temperature"),
            None,
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            &QualifierLosses::new(),
        );

        assert_eq!(summary.observed_at, None);
        assert_eq!(summary.unit_code, None);
        assert_eq!(summary.dataset_id, None);
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
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            &QualifierLosses::new(),
        );

        assert!(summary.observed_at.is_some());
        assert!(summary.unit_code.is_some());
        assert_eq!(summary.custom_attributes.len(), 1);
        assert!(matches!(attribute(&summary, "accuracy"), NgsiLdAttribute::Property(_)));
    }

    /// The summary for a `temperature` whose only qualifier is the `observedAt` `value`.
    fn summary_with_observed_at(value: Value, losses: &QualifierLosses) -> MetadataSummary {
        let mut shared = IndexMap::default();
        shared.insert(
            name("observedAt"),
            SubAttribute::new(NgsiLdAttributeKind::Property, AttributeContent::Value(value), IndexMap::default()),
        );
        let mut metadata = IndexMap::default();
        metadata.insert(name("temperature"), MetadataStorage::shared(shared));

        transform_metadata(
            Some(&metadata),
            &name("temperature"),
            None,
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            losses,
        )
    }

    #[test]
    fn a_numeric_epoch_observed_at_is_lifted_as_an_instant() {
        let losses = QualifierLosses::new();

        let summary = summary_with_observed_at(Value::from(1_775_253_620_i64), &losses);

        assert!(summary.observed_at.is_some());
        assert!(losses.timestamps.is_empty());
    }

    #[test]
    fn a_numeric_observed_at_that_is_no_epoch_is_recorded_rather_than_lost() {
        let losses = QualifierLosses::new();

        let summary = summary_with_observed_at(Value::from(2026_i64), &losses);

        assert_eq!(summary.observed_at, None);
        let entries = BatchSink::from(losses.timestamps).into_entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].0,
            AttributeLoss {
                loss: TimestampLoss::ObservedAt,
                attribute: name("temperature")
            }
        );
        assert_eq!(entries[0].1.example.as_ref(), "2026");
    }

    #[test]
    fn an_observed_at_extraction_already_read_as_an_instant_is_lifted_as_that_instant() {
        let losses = QualifierLosses::new();
        let instant = Utc.with_ymd_and_hms(2026, 4, 3, 22, 0, 20).unwrap();

        let summary = summary_with_observed_at(Value::Temporal(TemporalValue::DateTime(instant)), &losses);

        assert_eq!(summary.observed_at, Some(instant));
        assert!(losses.timestamps.is_empty());
    }

    #[test]
    fn a_blank_observed_at_is_absent_rather_than_unreadable() {
        let losses = QualifierLosses::new();

        let summary = summary_with_observed_at(Value::String("  ".into()), &losses);

        assert_eq!(summary.observed_at, None);
        assert!(losses.timestamps.is_empty());
    }

    /// The summary for a `temperature` whose only qualifier is the `expiresAt` `value`.
    fn summary_with_expires_at(value: Value, losses: &QualifierLosses) -> MetadataSummary {
        let mut shared = IndexMap::default();
        shared.insert(
            name("expiresAt"),
            SubAttribute::new(NgsiLdAttributeKind::Property, AttributeContent::Value(value), IndexMap::default()),
        );
        let mut metadata = IndexMap::default();
        metadata.insert(name("temperature"), MetadataStorage::shared(shared));

        transform_metadata(
            Some(&metadata),
            &name("temperature"),
            None,
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            losses,
        )
    }

    #[test]
    fn an_expires_at_is_lifted_as_an_instant_rather_than_left_a_sub_attribute() {
        let losses = QualifierLosses::new();

        let summary = summary_with_expires_at(Value::try_from(json!("2026-10-25T13:00:00Z")).unwrap(), &losses);

        assert_eq!(summary.expires_at, Some(Utc.with_ymd_and_hms(2026, 10, 25, 13, 0, 0).unwrap()));
        assert!(summary.custom_attributes.is_empty());
        assert!(losses.timestamps.is_empty());
    }

    #[test]
    fn an_expires_at_extraction_already_read_as_an_instant_is_lifted_as_that_instant() {
        let losses = QualifierLosses::new();
        let instant = Utc.with_ymd_and_hms(2026, 10, 25, 13, 0, 0).unwrap();

        let summary = summary_with_expires_at(Value::Temporal(TemporalValue::DateTime(instant)), &losses);

        assert_eq!(summary.expires_at, Some(instant));
        assert!(losses.timestamps.is_empty());
    }

    #[test]
    fn an_expires_at_that_reads_as_no_instant_is_recorded_under_its_own_loss() {
        let losses = QualifierLosses::new();

        let summary = summary_with_expires_at(Value::try_from(json!("tomorrow")).unwrap(), &losses);

        assert_eq!(summary.expires_at, None);
        assert!(summary.custom_attributes.is_empty());
        let entries = BatchSink::from(losses.timestamps).into_entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].0,
            AttributeLoss {
                loss: TimestampLoss::ExpiresAt,
                attribute: name("temperature")
            }
        );
        assert_eq!(entries[0].1.example.as_ref(), "tomorrow");
    }

    #[test]
    fn a_blank_expires_at_is_absent_rather_than_unreadable() {
        let losses = QualifierLosses::new();

        let summary = summary_with_expires_at(Value::String("  ".into()), &losses);

        assert_eq!(summary.expires_at, None);
        assert!(losses.timestamps.is_empty());
    }

    #[test]
    fn one_attribute_losing_both_qualifiers_records_each_under_its_own_loss() {
        let losses = QualifierLosses::new();
        let mut shared = IndexMap::default();
        shared.insert(name("observedAt"), sub(NgsiLdAttributeKind::Property, json!("nope")));
        shared.insert(name("expiresAt"), sub(NgsiLdAttributeKind::Property, json!("later")));
        let mut metadata = IndexMap::default();
        metadata.insert(name("temperature"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("temperature"),
            None,
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            &losses,
        );

        assert_eq!(summary.observed_at, None);
        assert_eq!(summary.expires_at, None);
        let entries = BatchSink::from(losses.timestamps).into_entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].0,
            AttributeLoss {
                loss: TimestampLoss::ObservedAt,
                attribute: name("temperature")
            }
        );
        assert_eq!(
            entries[1].0,
            AttributeLoss {
                loss: TimestampLoss::ExpiresAt,
                attribute: name("temperature")
            }
        );
    }

    /// The summary for a `pm10` whose only qualifier is the `unitCode` `value`.
    fn summary_with_unit_code(value: JsonValue, losses: &QualifierLosses) -> MetadataSummary {
        let mut shared = IndexMap::default();
        shared.insert(name("unitCode"), sub(NgsiLdAttributeKind::Property, value));
        let mut metadata = IndexMap::default();
        metadata.insert(name("pm10"), MetadataStorage::shared(shared));

        transform_metadata(
            Some(&metadata),
            &name("pm10"),
            None,
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            losses,
        )
    }

    /// Every unknown unit code `losses` recorded, as its attribute, its text, and how often it was met.
    fn unknown_unit_codes(losses: QualifierLosses) -> Vec<(String, String, u64)> {
        BatchSink::from(losses.unit_codes)
            .into_entries()
            .map(|(key, record)| (key.attribute.to_string(), key.text.into_string(), record.occurrences().get()))
            .collect()
    }

    fn unknown(attribute: &str, text: &str, occurrences: u64) -> (String, String, u64) {
        (attribute.to_owned(), text.to_owned(), occurrences)
    }

    #[test]
    fn a_recommendation_20_unit_code_is_kept() {
        let losses = QualifierLosses::new();

        let summary = summary_with_unit_code(json!("GQ"), &losses);

        assert_eq!(summary.unit_code, Some(UnitCode::Gq));
        assert!(losses.unit_codes.is_empty());
    }

    #[test]
    fn an_unknown_unit_code_is_left_out_and_recorded() {
        let losses = QualifierLosses::new();

        let summary = summary_with_unit_code(json!("ug/m3x"), &losses);

        assert_eq!(summary.unit_code, None);
        assert!(summary.custom_attributes.is_empty());
        assert_eq!(unknown_unit_codes(losses), [unknown("pm10", "ug/m3x", 1)]);
    }

    #[test]
    fn a_non_text_unit_code_is_recorded_by_its_json_text() {
        let losses = QualifierLosses::new();

        let summary = summary_with_unit_code(json!(5), &losses);

        assert_eq!(summary.unit_code, None);
        assert_eq!(unknown_unit_codes(losses), [unknown("pm10", "5", 1)]);
    }

    #[test]
    fn a_numeric_unit_code_whose_json_text_is_a_code_is_kept() {
        let losses = QualifierLosses::new();

        let summary = summary_with_unit_code(json!(59), &losses);

        assert_eq!(summary.unit_code, Some(UnitCode::Code59));
        assert!(losses.unit_codes.is_empty());
    }

    #[test]
    fn a_unit_code_is_read_without_the_whitespace_around_it() {
        let losses = QualifierLosses::new();

        let summary = summary_with_unit_code(json!(" GQ "), &losses);

        assert_eq!(summary.unit_code, Some(UnitCode::Gq));
        assert!(losses.unit_codes.is_empty());
    }

    #[test]
    fn a_blank_unit_code_is_absent_rather_than_unknown() {
        let losses = QualifierLosses::new();

        let summary = summary_with_unit_code(json!("  "), &losses);

        assert_eq!(summary.unit_code, None);
        assert!(losses.unit_codes.is_empty());
    }

    #[test]
    fn an_unknown_unit_code_on_a_sub_attribute_is_recorded_against_its_attribute() {
        let losses = QualifierLosses::new();
        let mut max = IndexMap::default();
        max.insert(name("unitCode"), sub(NgsiLdAttributeKind::Property, json!("x")));
        let mut shared = IndexMap::default();
        shared.insert(
            name("max"),
            SubAttribute::new(NgsiLdAttributeKind::Property, AttributeContent::Value(Value::try_from(json!(30)).unwrap()), max),
        );
        let mut metadata = IndexMap::default();
        metadata.insert(name("temperature"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("temperature"),
            None,
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            &losses,
        );

        let NgsiLdAttribute::Property(max) = attribute(&summary, "max") else {
            panic!("expected a property sub-attribute");
        };
        assert_eq!(max.unit_code, None);
        assert_eq!(unknown_unit_codes(losses), [unknown("temperature", "x", 1)]);
    }

    #[test]
    fn one_cache_shared_across_attributes_yields_the_same_summaries_as_fresh_ones() {
        let mut shared = IndexMap::default();
        shared.insert(name("observedAt"), sub(NgsiLdAttributeKind::Property, json!("2026-04-03T22:00:20Z")));
        shared.insert(name("unitCode"), sub(NgsiLdAttributeKind::Property, json!("KWH")));
        let mut metadata = IndexMap::default();
        metadata.insert(name("import"), MetadataStorage::shared(shared.clone()));
        metadata.insert(name("export"), MetadataStorage::shared(shared));

        let mut observed_at = InstantCache::new();
        let mut unit_codes = UnitCodeCache::new();
        let mut cache = QualifierCache::new(&mut observed_at, &mut unit_codes);
        let first = transform_metadata(Some(&metadata), &name("import"), None, &mut cache, &QualifierLosses::new());
        let second = transform_metadata(Some(&metadata), &name("export"), None, &mut cache, &QualifierLosses::new());
        let fresh = transform_metadata(
            Some(&metadata),
            &name("export"),
            None,
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            &QualifierLosses::new(),
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

        let mut observed_at = InstantCache::new();
        let mut unit_codes = UnitCodeCache::new();
        let mut cache = QualifierCache::new(&mut observed_at, &mut unit_codes);
        let energy = transform_metadata(Some(&metadata), &name("energy"), None, &mut cache, &QualifierLosses::new());
        let voltage = transform_metadata(Some(&metadata), &name("voltage"), None, &mut cache, &QualifierLosses::new());

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
                &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
                &QualifierLosses::new(),
            )
            .unit_code
        );
    }

    #[test]
    fn a_vocab_property_sub_attribute_keeps_its_kind() {
        let mut shared = IndexMap::default();
        shared.insert(
            name("level"),
            SubAttribute::new(NgsiLdAttributeKind::VocabProperty, vocab("https://example.org/level/high"), IndexMap::default()),
        );
        let mut metadata = IndexMap::default();
        metadata.insert(name("sugars"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("sugars"),
            None,
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            &QualifierLosses::new(),
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
            SubAttribute::new(
                NgsiLdAttributeKind::LanguageProperty,
                AttributeContent::Language(
                    [("en", "high"), ("fr", "eleve")]
                        .into_iter()
                        .map(|(tag, text)| (LangTagBuf::new(tag.to_owned()).unwrap(), text.to_owned()))
                        .collect(),
                ),
                IndexMap::default(),
            ),
        );
        let mut metadata = IndexMap::default();
        metadata.insert(name("sugars"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("sugars"),
            None,
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            &QualifierLosses::new(),
        );

        assert!(matches!(attribute(&summary, "tags"), NgsiLdAttribute::ListProperty(_)));
        assert!(matches!(attribute(&summary, "label"), NgsiLdAttribute::LanguageProperty(_)));
    }

    #[test]
    fn a_geo_property_sub_attribute_is_built_around_the_geometry_extraction_typed() {
        // A ring wound clockwise, as a `winding: keep` policy leaves it; building the sub-attribute
        // must not rewind it.
        let clockwise = NgsiLdGeometry::Polygon {
            coordinates: vec![vec![
                [0.0, 0.0].into(),
                [0.0, 2.0].into(),
                [2.0, 2.0].into(),
                [2.0, 0.0].into(),
                [0.0, 0.0].into(),
            ]],
        };
        let mut shared = IndexMap::default();
        shared.insert(
            name("footprint"),
            SubAttribute::new(
                NgsiLdAttributeKind::GeoProperty,
                AttributeContent::Value(Value::Geospatial(Box::new(clockwise.clone()))),
                IndexMap::default(),
            ),
        );
        let mut metadata = IndexMap::default();
        metadata.insert(name("name"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("name"),
            None,
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            &QualifierLosses::new(),
        );

        let NgsiLdAttribute::GeoProperty(footprint) = attribute(&summary, "footprint") else {
            panic!("expected a geo property sub-attribute");
        };
        assert_eq!(footprint.value, clockwise);
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
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            &QualifierLosses::new(),
        );

        assert!(matches!(attribute(&summary, "accuracy"), NgsiLdAttribute::Property(_)));
    }

    #[test]
    fn a_relationship_sub_attribute_becomes_a_relationship_with_object_type_and_nested_sub_attribute() {
        let mut inner = IndexMap::default();
        inner.insert(name("billingOrder"), sub(NgsiLdAttributeKind::Property, json!(0)));
        let relationship = SubAttribute::new_relationship(
            NgsiLdAttributeKind::Relationship,
            Value::try_from(json!("urn:ngsi-ld:Character:JackSparrow")).unwrap(),
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
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            &QualifierLosses::new(),
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
            SubAttribute::new(NgsiLdAttributeKind::VocabProperty, vocab("https://example.org/level/high"), inner),
        );
        let mut metadata = IndexMap::default();
        metadata.insert(name("sugars"), MetadataStorage::shared(shared));

        let summary = transform_metadata(
            Some(&metadata),
            &name("sugars"),
            None,
            &mut QualifierCache::new(&mut InstantCache::new(), &mut UnitCodeCache::new()),
            &QualifierLosses::new(),
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
