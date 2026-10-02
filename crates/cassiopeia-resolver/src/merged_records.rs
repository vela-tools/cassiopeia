use crate::record_merge::RecordMerge;
use ahash::AHashMap;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use std::num::NonZeroU64;

/// What the record merges of one entity type amount to across a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedType {
    /// The merge a report names: the id that absorbed the most records, the lowest id breaking a
    /// tie, so every run over the same source names the same id whatever order the ids were assembled
    /// in.
    pub exemplar: RecordMerge,
    /// How many ids of this type had disagreeing records, the exemplar included.
    pub ids: NonZeroU64,
}

impl MergedType {
    /// Counts one more merge of this type, keeping whichever exemplar ranks first.
    fn absorb(&mut self, merge: RecordMerge) {
        self.ids = self.ids.saturating_add(1);
        // More records ranks first; on equal records the lower id does, which is why the two ids sit
        // on opposite sides of the comparison.
        let ranks_first = (merge.records, &self.exemplar.id) > (self.exemplar.records, &merge.id);
        if ranks_first {
            self.exemplar = merge;
        }
    }
}

/// The disagreeing record merges one assembly scan found, grouped by the type of the entity they
/// merged into.
///
/// Grouping by type keeps a source that repeats ten thousand ids to one line per type: the type and
/// the code are the failure, and the exemplar id and the counts ride along. Every quantity is a sum
/// or an order-independent pick, so the scan's parallelism and the store's id order cannot change
/// what is reported.
#[derive(Debug, Default)]
pub struct MergedRecords {
    by_type: AHashMap<NameBuf, MergedType>,
}

impl MergedRecords {
    /// Opens an empty tally.
    #[must_use]
    pub fn new() -> MergedRecords {
        MergedRecords::default()
    }

    /// Records one id's merge under its entity type.
    pub fn record(&mut self, merge: RecordMerge) {
        if let Some(merged) = self.by_type.get_mut(&merge.entity_type) {
            merged.absorb(merge);
            return;
        }

        // A type seen for the first time is the only case that owns a second copy of its name: the
        // map key and the exemplar both keep one.
        self.by_type.insert(
            merge.entity_type.clone(),
            MergedType {
                exemplar: merge,
                ids: NonZeroU64::MIN,
            },
        );
    }

    /// Whether the scan found no disagreeing records at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_type.is_empty()
    }

    /// Every type's merges, ordered by type name so a report lists them the same way every run.
    #[must_use]
    pub fn into_entries(self) -> Vec<MergedType> {
        let mut entries: Vec<MergedType> = self.by_type.into_values().collect();
        entries.sort_by(|left, right| left.exemplar.entity_type.as_str().cmp(right.exemplar.entity_type.as_str()));
        entries
    }
}

#[cfg(test)]
mod tests {
    use crate::{field_path::FieldPath, merged_records::MergedRecords, record_merge::RecordMerge};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use std::num::NonZeroU64;

    fn merge(entity_type: &str, id: &str, records: u64) -> RecordMerge {
        RecordMerge {
            id: format!("urn:ngsi-ld:{entity_type}:{id}").parse().unwrap(),
            entity_type: NameBuf::new(entity_type).unwrap(),
            records: NonZeroU64::new(records).unwrap(),
            field: FieldPath::new(&["temperature"]),
        }
    }

    #[test]
    fn a_fresh_tally_holds_nothing() {
        assert!(MergedRecords::new().is_empty());
        assert!(MergedRecords::new().into_entries().is_empty());
    }

    #[test]
    fn merges_of_one_type_collapse_into_one_entry_counting_every_id() {
        let mut tally = MergedRecords::new();
        for index in 0..10_000 {
            tally.record(merge("Station", &format!("s{index}"), 2));
        }

        let entries = tally.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].ids.get(), 10_000);
    }

    #[test]
    fn the_exemplar_is_the_id_with_the_most_records_whatever_order_they_arrive_in() {
        let merges = [merge("Station", "c", 2), merge("Station", "b", 7), merge("Station", "a", 3)];
        for rotation in 0..merges.len() {
            let mut tally = MergedRecords::new();
            for index in 0..merges.len() {
                tally.record(merges[(index + rotation) % merges.len()].clone());
            }

            let entries = tally.into_entries();
            assert_eq!(entries[0].exemplar, merge("Station", "b", 7));
            assert_eq!(entries[0].ids.get(), 3);
        }
    }

    #[test]
    fn a_tie_on_records_names_the_lowest_id_whatever_order_they_arrive_in() {
        for order in [["b", "a"], ["a", "b"]] {
            let mut tally = MergedRecords::new();
            for id in order {
                tally.record(merge("Station", id, 2));
            }

            assert_eq!(tally.into_entries()[0].exemplar, merge("Station", "a", 2));
        }
    }

    #[test]
    fn each_type_is_its_own_entry_listed_by_type_name() {
        let mut tally = MergedRecords::new();
        tally.record(merge("WeatherStation", "w", 2));
        tally.record(merge("BikeRentalShop", "b", 2));

        let types: Vec<String> = tally.into_entries().into_iter().map(|entry| entry.exemplar.entity_type.to_string()).collect();
        assert_eq!(types, ["BikeRentalShop", "WeatherStation"]);
    }
}
