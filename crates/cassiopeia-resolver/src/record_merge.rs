use crate::field_path::FieldPath;
use cassiopeia_mapping::{mapping::Mapping, mapping_role::MappingRole};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use std::num::NonZeroU64;
use urn_rs::Urn;

/// One entity id that several records of the same mapping resolved to and disagreed on, so that
/// merging them into a single entity discarded a value.
///
/// A record is a description of one entity, so two records of one mapping landing on one id with
/// different values for the same field are either the same entity described inconsistently or two
/// entities the identity failed to tell apart; either way the entity written keeps one value and
/// drops the other, which is what the author has to be told. Records that only add fields to one
/// another, or repeat one another exactly, lose nothing and are not a merge worth reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordMerge {
    /// The entity id the records resolved to.
    pub id: Urn,
    /// The type of the entity the records were merged into.
    pub entity_type: NameBuf,
    /// How many records the disagreeing mappings resolved to the id.
    pub records: NonZeroU64,
    /// The earliest source field the records disagreed on, the same whichever order they arrived in.
    pub field: FieldPath,
}

impl RecordMerge {
    /// Finds the record merge among the contributions to one id: each a mapping, how many of its
    /// records resolved to the id, and the earliest field those records disagreed on.
    ///
    /// Only the records of a mapping that [merges records](RecordMerge::counts_records_of) count, and
    /// only when they disagreed: several mappings contributing to one id are a join, deliberate by
    /// construction, and each mapping's records are compared only with one another. The merge stands
    /// for every record of the disagreeing mappings, under the entity type of the first of them, and
    /// names the earliest field any of them disagreed on.
    #[must_use]
    pub fn among<'a>(id: &Urn, contributions: impl IntoIterator<Item = (&'a Mapping, NonZeroU64, Option<FieldPath>)>) -> Option<RecordMerge> {
        let mut merge: Option<RecordMerge> = None;
        for (mapping, records, conflict) in contributions {
            let Some(field) = conflict.filter(|_| RecordMerge::counts_records_of(mapping)) else {
                continue;
            };
            match &mut merge {
                Some(found) => {
                    found.records = found.records.saturating_add(records.get());
                    if field < found.field {
                        found.field = field;
                    }
                }
                None => {
                    merge = Some(RecordMerge {
                        // The merge outlives the borrowed id and mapping the assembly scan holds.
                        id: id.clone(),
                        entity_type: mapping.data_model().entity_type().clone(),
                        records,
                        field,
                    });
                }
            }
        }
        merge
    }

    /// Whether several of `mapping`'s records resolving to one id is a merge worth reporting.
    ///
    /// Two kinds of mapping fold many records into one entity on purpose. A temporal mapping (one
    /// declaring `observedAt`) reads each record as one observation of the entity: the current-state
    /// store keeps the latest and the series store keeps them all (ETSI GS CIM 009 v1.9.1 clause
    /// 4.5.5 with 5.2.20). A `syntheticEntity` emits the entity a record names, which every record
    /// naming the same target names too. Every other mapping's records each describe an entity of
    /// their own, so a second record disagreeing on the same id is reported.
    #[must_use]
    pub fn counts_records_of(mapping: &Mapping) -> bool {
        match mapping.role() {
            MappingRole::Document => !mapping.is_temporal(),
            MappingRole::Synthetic => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{field_path::FieldPath, record_merge::RecordMerge};
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use std::{num::NonZeroU64, path::Path};
    use urn_rs::Urn;

    const STATIC: &str = r#"{
        version: "v4",
        dataModel: "Station",
        identity: { entityName: "{{ id }}" },
        attributes: { temperature: { source: "{{ temperature }}" } },
    }"#;

    const TEMPORAL: &str = r#"{
        version: "v4",
        dataModel: "Station",
        identity: { entityName: "{{ id }}" },
        attributes: { temperature: { source: "{{ temperature }}", properties: { observedAt: { source: "{{ ts }}" } } } },
    }"#;

    const WITH_SYNTHETIC: &str = r#"{
        version: "v4",
        dataModel: "Mountain",
        identity: { entityName: "{{ name }}" },
        attributes: {
            hasCountry: {
                source: "{{ country }}",
                type: "Relationship",
                target: { entity: "Country" },
                syntheticEntity: { dataModel: "Country", identity: { entityName: "{{ country }}" }, attributes: {} },
            },
        },
    }"#;

    fn mapping(document: &str) -> Mapping {
        Mapping::from_json5(document, Path::new("test.json5"), &mut TemplateRunner::new()).unwrap()
    }

    fn records(count: u64) -> NonZeroU64 {
        NonZeroU64::new(count).unwrap()
    }

    fn field(name: &str) -> FieldPath {
        FieldPath::new(&[name])
    }

    fn station() -> Urn {
        "urn:ngsi-ld:Station:a".parse().unwrap()
    }

    #[test]
    fn disagreeing_records_of_one_static_mapping_are_a_merge_naming_the_field() {
        let static_mapping = mapping(STATIC);

        let merge = RecordMerge::among(&station(), [(&static_mapping, records(2), Some(field("temperature")))]).unwrap();

        assert_eq!(merge.id, station());
        assert_eq!(merge.entity_type.as_str(), "Station");
        assert_eq!(merge.records.get(), 2);
        assert_eq!(merge.field.to_string(), "temperature");
    }

    #[test]
    fn records_of_one_static_mapping_that_never_disagreed_are_no_merge() {
        let static_mapping = mapping(STATIC);

        assert_eq!(RecordMerge::among(&station(), [(&static_mapping, records(5), None)]), None);
    }

    #[test]
    fn a_join_whose_one_side_disagrees_counts_only_that_side() {
        let joined = mapping(STATIC);
        let disagreeing = mapping(STATIC);

        let merge = RecordMerge::among(
            &station(),
            [(&joined, records(1), None), (&disagreeing, records(3), Some(field("temperature")))],
        )
        .unwrap();

        assert_eq!(merge.records.get(), 3);
    }

    #[test]
    fn two_disagreeing_mappings_name_the_earliest_field_either_disagreed_on() {
        let first = mapping(STATIC);
        let second = mapping(STATIC);

        let merge = RecordMerge::among(&station(), [(&first, records(2), Some(field("z"))), (&second, records(2), Some(field("a")))]).unwrap();

        assert_eq!(merge.records.get(), 4);
        assert_eq!(merge.field.to_string(), "a");
    }

    #[test]
    fn a_temporal_mapping_folding_many_observations_is_no_merge() {
        let temporal = mapping(TEMPORAL);

        assert_eq!(RecordMerge::among(&station(), [(&temporal, records(12), Some(field("temperature")))]), None);
    }

    #[test]
    fn a_synthetic_entity_named_by_many_records_is_no_merge() {
        let mountain = mapping(WITH_SYNTHETIC);
        let country = mountain
            .attributes()
            .values()
            .find_map(|attribute| attribute.synthetic_entity().as_ref())
            .unwrap();
        let nepal: Urn = "urn:ngsi-ld:Country:Nepal".parse().unwrap();

        assert_eq!(RecordMerge::among(&nepal, [(country, records(14), Some(field("name")))]), None);
    }
}
