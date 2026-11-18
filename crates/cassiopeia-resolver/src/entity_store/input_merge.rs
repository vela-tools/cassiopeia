use crate::{
    entity_store::{mapping_expiry::MappingExpiry, merge::deep_merge},
    field_path::FieldPath,
};
use cassiopeia_mapping::source_reads::SourceReads;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use smallvec::SmallVec;

/// The records one input of a run gave a static mapping for one id, merged into one in the order the
/// input lists them.
///
/// A field only some of the records hold is taken from them, and a field they disagree on keeps the
/// value of the record merged first (see [`deep_merge`]), so records are merged in the order of the
/// source. A run reads its inputs side by side, so the records of two inputs that share a mapping
/// reach the store interleaved in whatever order they are ready, while the records of one input reach
/// it in the order of the input. Each input's records are therefore merged apart, and the inputs'
/// merges are [`join`]ed in the order of the manifest once every record has arrived.
///
/// The input's place sits beside the merged record's own fields, where it fills the padding they
/// leave, so a merge takes no more room for knowing its input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct InputMerge {
    /// The place in the manifest of the input that read the records.
    input: u32,
    /// The merged record.
    data: Value,
    /// The earliest field the mapping reads on which the merged records disagreed.
    conflict: Option<FieldPath>,
    /// The entity `expiresAt` the merged records contribute.
    expires_at: MappingExpiry,
}

/// The merges of every input that gave a static mapping records for one id, in the order of the
/// inputs in the manifest. One in the common case of an id that only one input names, held without a
/// heap allocation.
pub(crate) type InputMerges = SmallVec<[InputMerge; 1]>;

impl InputMerge {
    /// What no record of the input at `input` in the manifest has been merged into yet.
    pub(crate) fn nothing(input: u32) -> InputMerge {
        InputMerge {
            input,
            data: Value::Object(Map::new()),
            conflict: None,
            expires_at: MappingExpiry::first(None),
        }
    }

    /// The record `data` of the input at `input` in the manifest, declaring the entity `expiresAt`
    /// `expires_at`, merged with nothing.
    pub(crate) const fn record(input: u32, data: Value, expires_at: Option<DateTime<Utc>>) -> InputMerge {
        InputMerge {
            input,
            data,
            conflict: None,
            expires_at: MappingExpiry::first(expires_at),
        }
    }

    /// The place in the manifest of the input that read the records.
    pub(crate) const fn input(&self) -> u32 {
        self.input
    }

    /// Merges `later`, records that all come after the ones merged so far in the source, whether of
    /// this input or of a later one, into them, counting a disagreement only beneath a key in `reads`.
    ///
    /// The merged records keep the disagreements found among them, and the disagreements between the
    /// two sides are added to them.
    pub(crate) fn absorb(&mut self, later: InputMerge, reads: &SourceReads) {
        let InputMerge {
            data, conflict, expires_at, ..
        } = later;
        if self.data.as_object().is_some_and(Map::is_empty) {
            // Merging into nothing yields the later records themselves, so they are moved in rather
            // than copied field by field.
            self.data = data;
        } else {
            let between = deep_merge(&mut self.data, &data, reads);
            self.conflict = FieldPath::earliest(self.conflict.take(), between);
        }
        self.conflict = FieldPath::earliest(self.conflict.take(), conflict);
        self.expires_at.refine(expires_at);
    }
}

/// Joins `merges`, which are in the order of their inputs in the manifest, into the one record the
/// mapping keeps for the id, counting a disagreement only beneath a key in `reads`: the merged record,
/// the earliest field the mapping reads on which any of the records disagreed, and the expiry they
/// contribute.
///
/// Each input's merge is merged after the merges of the inputs before it, so a field the inputs
/// disagree on keeps the value of the first input in the manifest that holds it, and within that
/// input of its first record that holds it, as it would were every record merged in the order of the
/// source. A disagreement between inputs counts as one between the records of one input does, and
/// the expiry is joined as the records of one input join theirs. No input, which no store keeps a
/// slot for, joins into an empty record.
pub(crate) fn join(merges: InputMerges, reads: &SourceReads) -> (Value, Option<FieldPath>, MappingExpiry) {
    let mut merges = merges.into_iter();
    let Some(mut joined) = merges.next() else {
        return (Value::Object(Map::new()), None, MappingExpiry::first(None));
    };
    for later in merges {
        joined.absorb(later, reads);
    }
    let InputMerge {
        data, conflict, expires_at, ..
    } = joined;
    (data, conflict, expires_at)
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{
            input_merge::{InputMerge, InputMerges, join},
            mapping_expiry::MappingExpiry,
        },
        field_path::FieldPath,
    };
    use cassiopeia_mapping::source_reads::{SourceKey, SourceReads};
    use chrono::{DateTime, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use smallvec::smallvec;

    fn instant(seconds: i64) -> Option<DateTime<Utc>> {
        Utc.timestamp_opt(seconds, 0).single()
    }

    /// The input at `input` in the manifest, which gave `records`, in order, each with its expiry.
    fn input_merge(input: u32, records: &[(Value, Option<DateTime<Utc>>)]) -> InputMerge {
        let mut merge = InputMerge::nothing(input);
        for (data, expires_at) in records {
            merge.absorb(InputMerge::record(input, data.clone(), *expires_at), &SourceReads::Everything);
        }
        merge
    }

    #[test]
    fn nothing_merged_with_one_record_is_the_record_and_its_expiry() {
        assert_eq!(
            input_merge(0, &[(json!({"capacity": 40}), instant(100))]),
            InputMerge::record(0, json!({"capacity": 40}), instant(100))
        );
    }

    #[test]
    fn a_disagreeing_field_keeps_the_value_of_the_record_merged_first_and_is_the_conflict() {
        let merges: InputMerges = smallvec![input_merge(
            0,
            &[(json!({"capacity": 40, "name": "Main"}), None), (json!({"capacity": 42, "zone": "B"}), None)]
        )];

        assert_eq!(
            join(merges, &SourceReads::Everything),
            (
                json!({"capacity": 40, "name": "Main", "zone": "B"}),
                Some(FieldPath::new(&["capacity"])),
                MappingExpiry::first(None)
            )
        );
    }

    #[test]
    fn inputs_merged_apart_and_then_joined_keep_what_merging_every_record_one_by_one_keeps() {
        let first = [
            (json!({"capacity": 40, "zone": "A"}), instant(300)),
            (json!({"capacity": 41, "name": "Main"}), None),
        ];
        let second = [
            (json!({"capacity": 42, "name": "Renamed", "level": 2}), instant(100)),
            (json!({"level": 3, "zone": "C"}), None),
        ];
        let one_by_one = input_merge(0, &[first[0].clone(), first[1].clone(), second[0].clone(), second[1].clone()]);

        let joined = join(smallvec![input_merge(0, &first), input_merge(1, &second)], &SourceReads::Everything);

        assert_eq!(joined, join(smallvec![one_by_one], &SourceReads::Everything));
        assert_eq!(
            joined,
            (
                json!({"capacity": 40, "zone": "A", "name": "Main", "level": 2}),
                Some(FieldPath::new(&["capacity"])),
                MappingExpiry::first(instant(100))
            )
        );
    }

    #[test]
    fn a_field_the_inputs_disagree_on_keeps_the_first_input_s_value_and_a_field_only_a_later_one_holds_is_taken_from_it() {
        let merges: InputMerges = smallvec![
            input_merge(0, &[(json!({"name": "Main", "capacity": 40}), instant(300))]),
            input_merge(1, &[(json!({"name": "Main", "capacity": 42, "zone": "B"}), None)]),
            input_merge(2, &[(json!({"capacity": 44, "zone": "C", "level": 3}), instant(100))]),
        ];

        assert_eq!(
            join(merges, &SourceReads::Everything),
            (
                json!({"name": "Main", "capacity": 40, "zone": "B", "level": 3}),
                Some(FieldPath::new(&["capacity"])),
                MappingExpiry::first(instant(100))
            )
        );
    }

    #[test]
    fn the_earliest_conflict_is_kept_whichever_input_found_it() {
        let conflicting = |input: u32, field: &str| input_merge(input, &[(json!({ field: 1 }), None), (json!({ field: 2 }), None)]);

        assert_eq!(
            join(smallvec![conflicting(0, "a"), conflicting(1, "z")], &SourceReads::Everything).1,
            Some(FieldPath::new(&["a"]))
        );
        assert_eq!(
            join(smallvec![conflicting(0, "z"), conflicting(1, "a")], &SourceReads::Everything).1,
            Some(FieldPath::new(&["a"]))
        );
        let across: InputMerges = smallvec![
            input_merge(0, &[(json!({"b": 1, "z": 1}), None), (json!({"z": 2}), None)]),
            input_merge(1, &[(json!({"b": 2, "y": 1}), None), (json!({"y": 2}), None)]),
        ];
        assert_eq!(join(across, &SourceReads::Everything).1, Some(FieldPath::new(&["b"])));
    }

    #[test]
    fn inputs_that_only_add_fields_to_one_another_join_without_a_conflict() {
        let merges: InputMerges = smallvec![
            input_merge(0, &[(json!({"name": "Main"}), None)]),
            input_merge(1, &[(json!({"name": "Main", "capacity": 40}), None)]),
        ];

        assert_eq!(
            join(merges, &SourceReads::Everything),
            (json!({"name": "Main", "capacity": 40}), None, MappingExpiry::first(None))
        );
    }

    #[test]
    fn a_disagreement_between_inputs_counts_only_beneath_a_read_key() {
        let reads = SourceReads::Keys([SourceKey::new("capacity")].into_iter().collect());
        let merges: InputMerges = smallvec![
            InputMerge::record(0, json!({"capacity": 40, "level": 2}), None),
            InputMerge::record(1, json!({"capacity": 40, "level": 10}), None),
        ];

        assert_eq!(join(merges, &reads), (json!({"capacity": 40, "level": 2}), None, MappingExpiry::first(None)));
    }

    #[test]
    fn no_input_joins_into_an_empty_record() {
        assert_eq!(
            join(InputMerges::new(), &SourceReads::Everything),
            (json!({}), None, MappingExpiry::first(None))
        );
    }

    #[test]
    fn an_input_merge_survives_the_store_encoding() {
        let merge = input_merge(3, &[(json!({"capacity": 40}), instant(100)), (json!({"capacity": 42}), None)]);

        let encoded = rmp_serde::to_vec(&merge).unwrap();

        assert_eq!(rmp_serde::from_slice::<InputMerge>(&encoded).unwrap(), merge);
        assert_eq!(merge.input(), 3);
    }
}
