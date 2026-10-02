use crate::field_path::FieldPath;
use cassiopeia_mapping::source_reads::SourceReads;
use serde_json::Value;

/// Recursively merges `src` into `dst`, returning the earliest field the mapping reads on which `src`
/// disagreed with what `dst` already held.
///
/// For objects, keys from `src` are added to `dst` without overwriting existing keys, except nested
/// objects, which merge recursively. Any other pair keeps the existing value; when the two differ,
/// `src`'s value is discarded and that field is a conflict. Only brand-new keys pay a clone: existing
/// keys are looked up by reference.
///
/// What counts as a conflict:
/// - only a field beneath a top-level key in `reads`: a field the mapping never reads cannot change
///   what it emits, so losing it loses nothing (the merge itself still keeps every field the same
///   way);
/// - only a field both sides hold: a key only `src` carries is added, never a conflict;
/// - only where the two are not both objects: two objects merge key by key, so only their leaves can
///   disagree;
/// - any two unequal values otherwise, compared as strict JSON: an array is kept or discarded whole,
///   so any difference in it is a conflict, and `null` is a value like any other.
///
/// `null` has to count. The existing value is kept even when it is `null`, so a `null` arriving first
/// discards a later reading, and the same pair arriving the other way round keeps it: excusing `null`
/// in either direction would make whether a conflict is found depend on arrival order.
///
/// Counted this way, the earliest conflicting field (in [`FieldPath`] order) is the same whichever
/// order the records arrive in. A field is found in conflict only where two records hold unequal,
/// not-both-object values beneath keys that are objects in both, and the earliest such field in the
/// whole set of records is always met: every record reaching it finds objects above it, since an
/// earlier non-object there would itself be an earlier conflict. Every record is an object at its
/// root, so each top-level key's subtree is merged on its own; leaving out the subtrees of unread keys
/// leaves every other subtree's earliest conflict exactly as it was.
#[must_use]
pub(crate) fn deep_merge(dst: &mut Value, src: &Value, reads: &SourceReads) -> Option<FieldPath> {
    let mut path = Vec::new();
    merge_at(dst, src, &mut path, reads)
}

/// The earliest field the mapping reads on which any of `records` disagree, found by merging them
/// exactly as a current-state store would.
///
/// The first record is copied once as the accumulator the others merge into, so this costs what
/// storing the records would have; the caller only asks it of records it already holds side by side.
#[must_use]
pub(crate) fn first_conflict<'a>(records: impl IntoIterator<Item = &'a Value>, reads: &SourceReads) -> Option<FieldPath> {
    let mut records = records.into_iter();
    // The accumulator is a scratch copy: the records themselves are still to be emitted unchanged.
    let mut merged = records.next()?.clone();
    records.fold(None, |earliest, record| FieldPath::earliest(earliest, deep_merge(&mut merged, record, reads)))
}

/// Merges `src` into `dst`, both found at `path`, returning the earliest conflict beneath it on a field
/// `reads` covers.
fn merge_at<'a>(dst: &mut Value, src: &'a Value, path: &mut Vec<&'a str>, reads: &SourceReads) -> Option<FieldPath> {
    match (dst, src) {
        (Value::Object(dst_map), Value::Object(src_map)) => {
            let mut earliest = None;
            for (key, src_val) in src_map {
                if let Some(existing) = dst_map.get_mut(key.as_str()) {
                    path.push(key);
                    earliest = FieldPath::earliest(earliest, merge_at(existing, src_val, path, reads));
                    path.pop();
                } else {
                    dst_map.insert(key.clone(), src_val.clone());
                }
            }
            earliest
        }
        (kept, offered) => (*kept != *offered && path.first().is_none_or(|key| reads.reads(key))).then(|| FieldPath::new(path)),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::merge::{deep_merge, first_conflict},
        field_path::FieldPath,
    };
    use cassiopeia_mapping::source_reads::{SourceKey, SourceReads};
    use serde_json::{Value, json};

    fn merged(mut dst: Value, src: &Value) -> (Value, Option<String>) {
        let conflict = deep_merge(&mut dst, src, &SourceReads::Everything);
        (dst, conflict.map(|path| path.to_string()))
    }

    #[test]
    fn disjoint_fields_are_added_without_a_conflict() {
        assert_eq!(
            merged(json!({"lat": 46, "temperature": 20}), &json!({"lat": 46, "wind": 3})),
            (json!({"lat": 46, "temperature": 20, "wind": 3}), None)
        );
    }

    #[test]
    fn an_identical_record_merges_without_a_conflict() {
        let record = json!({"id": "a", "position": {"type": "Point", "coordinates": [14.5, 46.0]}, "tags": [1, 2]});

        assert_eq!(merged(record.clone(), &record), (record, None));
    }

    #[test]
    fn a_differing_scalar_keeps_the_existing_value_and_names_the_field() {
        assert_eq!(
            merged(json!({"temperature": 20}), &json!({"temperature": 21})),
            (json!({"temperature": 20}), Some("temperature".to_string()))
        );
    }

    #[test]
    fn only_a_leaf_beneath_nested_objects_conflicts_and_the_path_leads_to_it() {
        assert_eq!(
            merged(
                json!({"shop": {"geometry": {"type": "Point", "coordinates": [1, 2]}}}),
                &json!({"shop": {"geometry": {"type": "Point", "coordinates": [3, 4]}, "name": "x"}})
            ),
            (
                json!({"shop": {"geometry": {"type": "Point", "coordinates": [1, 2]}, "name": "x"}}),
                Some("shop.geometry.coordinates".to_string())
            )
        );
    }

    #[test]
    fn an_array_is_compared_whole_so_any_difference_conflicts() {
        assert_eq!(merged(json!({"tags": [1, 2]}), &json!({"tags": [1, 2, 3]})).1, Some("tags".to_string()));
        assert_eq!(merged(json!({"tags": [1, 2]}), &json!({"tags": [2, 1]})).1, Some("tags".to_string()));
    }

    #[test]
    fn null_against_a_value_conflicts_in_either_direction() {
        assert_eq!(merged(json!({"t": null}), &json!({"t": 20})), (json!({"t": null}), Some("t".to_string())));
        assert_eq!(merged(json!({"t": 20}), &json!({"t": null})), (json!({"t": 20}), Some("t".to_string())));
        assert_eq!(merged(json!({"t": null}), &json!({"t": null})).1, None);
    }

    #[test]
    fn an_object_against_a_scalar_conflicts_at_the_object_s_key() {
        assert_eq!(merged(json!({"a": {"b": 1}}), &json!({"a": 5})).1, Some("a".to_string()));
        assert_eq!(merged(json!({"a": 5}), &json!({"a": {"b": 1}})).1, Some("a".to_string()));
    }

    #[test]
    fn numbers_compare_as_strict_json_so_an_integer_and_a_float_differ() {
        assert_eq!(merged(json!({"t": 20}), &json!({"t": 20.0})).1, Some("t".to_string()));
    }

    #[test]
    fn the_earliest_of_several_conflicts_is_named() {
        assert_eq!(
            merged(json!({"b": 1, "a": {"z": 1, "y": 1}}), &json!({"b": 2, "a": {"z": 2, "y": 2}})).1,
            Some("a.y".to_string())
        );
    }

    #[test]
    fn the_earliest_conflict_among_records_is_the_same_in_every_arrival_order() {
        // Whether `q.a` is ever compared depends on whether the scalar `q` arrives first, yet `q` itself
        // conflicts in every order and precedes both `q.a` and `z`.
        let records = [json!({"q": {"a": 1}, "z": 1}), json!({"q": 5, "z": 1}), json!({"q": {"a": 2}, "z": 2})];
        for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            assert_eq!(
                first_conflict(order.iter().map(|index| &records[*index]), &SourceReads::Everything),
                Some(FieldPath::new(&["q"])),
                "{order:?}"
            );
        }
    }

    #[test]
    fn three_records_where_only_one_disagrees_conflict_in_every_arrival_order() {
        let records = [json!({"t": 1, "x": 1}), json!({"t": 1, "y": 2}), json!({"t": 2})];
        for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            assert_eq!(
                first_conflict(order.iter().map(|index| &records[*index]), &SourceReads::Everything),
                Some(FieldPath::new(&["t"])),
                "{order:?}"
            );
        }
    }

    #[test]
    fn records_that_only_add_fields_have_no_conflict_and_one_record_alone_has_none() {
        let pivot = [json!({"cell": "a", "t": 1}), json!({"cell": "a", "wind": 2}), json!({"cell": "a", "rain": 3})];

        assert_eq!(first_conflict(&pivot, &SourceReads::Everything), None);
        assert_eq!(first_conflict(&pivot[..1], &SourceReads::Everything), None);
        assert_eq!(first_conflict(&[] as &[Value], &SourceReads::Everything), None);
    }

    fn reading(keys: &[&str]) -> SourceReads {
        SourceReads::Keys(keys.iter().map(|key| SourceKey::new(key)).collect())
    }

    #[test]
    fn a_conflict_on_a_key_the_mapping_does_not_read_does_not_count_yet_merges_the_same() {
        let mut dst = json!({"cell": "a", "level": 2, "temperature": 280.1});

        let conflict = deep_merge(
            &mut dst,
            &json!({"cell": "a", "level": 10, "wind": 3.0}),
            &reading(&["cell", "temperature", "wind"]),
        );

        assert_eq!(conflict, None);
        assert_eq!(dst, json!({"cell": "a", "level": 2, "temperature": 280.1, "wind": 3.0}));
    }

    #[test]
    fn a_conflict_on_a_read_key_counts_and_the_earliest_read_field_is_named() {
        let mut dst = json!({"a": 1, "b": {"x": 1}, "c": 1});

        let conflict = deep_merge(&mut dst, &json!({"a": 2, "b": {"x": 2}, "c": 2}), &reading(&["b", "c"]));

        assert_eq!(conflict.map(|path| path.to_string()), Some("b.x".to_string()));
    }

    #[test]
    fn records_differing_on_an_unread_level_conflict_only_once_the_mapping_reads_it() {
        let records = [
            json!({"cell": "srbw", "level": 2, "temperature": 280.1}),
            json!({"cell": "srbw", "level": 10, "wind_u": 3.2}),
        ];

        assert_eq!(first_conflict(&records, &reading(&["cell", "temperature", "wind_u"])), None);
        assert_eq!(
            first_conflict(&records, &reading(&["cell", "level", "temperature", "wind_u"])),
            Some(FieldPath::new(&["level"]))
        );
        assert_eq!(first_conflict(&records, &SourceReads::Everything), Some(FieldPath::new(&["level"])));
    }

    #[test]
    fn the_earliest_read_conflict_is_the_same_in_every_arrival_order_when_unread_keys_disagree_too() {
        // `a` disagrees but is not read; `q` shields `q.a` in some orders; `z` disagrees everywhere.
        let records = [
            json!({"a": 1, "q": {"a": 1}, "z": 1}),
            json!({"a": 2, "q": 5, "z": 1}),
            json!({"a": 3, "q": {"a": 2}, "z": 2}),
        ];
        let reads = reading(&["q", "z"]);
        for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            assert_eq!(
                first_conflict(order.iter().map(|index| &records[*index]), &reads),
                Some(FieldPath::new(&["q"])),
                "{order:?}"
            );
        }
        let only_z = reading(&["z"]);
        for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            assert_eq!(
                first_conflict(order.iter().map(|index| &records[*index]), &only_z),
                Some(FieldPath::new(&["z"])),
                "{order:?}"
            );
        }
    }
}
