use crate::{
    entity_store::{
        discarded_timestamps::DiscardedTimestamps,
        error::Result,
        mapping_reads::MappingReads,
        observations::RecordRanking,
        retention::Retention,
        write_batch::WriteBatch,
    },
    field_path::FieldPath,
    mapping_id::MappingId,
    store_write_strategy::StoreWriteStrategy,
};
use cassiopeia_ir::{observed_records::ObservedRecords, source_position::SourcePosition};
use cassiopeia_mapping::source_reads::SourceReads;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use chrono::{DateTime, Utc};
use serde_json::Value;
use smallvec::SmallVec;
use std::{fmt::Debug, num::NonZeroU64};
use urn_rs::Urn;

/// The inputs for storing one fragment under its base entity id.
///
/// Temporality is an attribute concern (ETSI GS CIM 009 v1.9.1 clause 4.5.5), so the store keys by
/// the base id and every mapping and every observation of that id accumulates together. `ranking`
/// lets a current-state store rank a temporal mapping's records by their `observedAt`, attribute by
/// attribute; a series store ignores it and keeps every observation.
///
/// The write owns the record it carries. The resolver already owns the fragment it is storing and
/// has no use for it afterwards, so handing ownership over lets an in-memory store move the record
/// into place instead of deep-copying a whole `serde_json` document per fragment. The mapping's read
/// set is only borrowed: the mapping outlives the write.
pub struct FragmentWrite<'a> {
    /// The entity id this fragment contributes to, with no temporal qualifier.
    pub base_id: Urn,
    /// The source record the fragment was expanded from.
    pub source_data: Value,
    /// Where that record stands in the run's sources. Writes reach a store in whatever order the
    /// resolver's threads finish them, so a store that keeps records in order keeps them in this one.
    pub position: SourcePosition,
    /// The batch the write belongs to: every write of an earlier batch reached the store before it.
    pub batch: WriteBatch,
    /// The interned id of the mapping that produced the fragment.
    pub mapping_id: MappingId,
    /// What the record ranks by against the mapping's other records for the id: nothing, for a
    /// mapping without `observedAt`, or the instants its `observedAt` templates name.
    pub ranking: RecordRanking,
    /// The entity's scope, when one was resolved.
    pub scope: Option<NgsiLdScope>,
    /// The entity's `expiresAt`, when one was resolved (ETSI GS CIM 009 v1.9.1 clause 4.5.1).
    pub expires_at: Option<DateTime<Utc>>,
    /// The top-level source keys the producing mapping reads: a current-state store counts a
    /// disagreement between merged records only beneath one of them.
    pub reads: &'a SourceReads,
}

/// One fragment's rendering inputs, read back at assembly.
///
/// The extractor resolves the record through the mapping the id resolves to, so only the record, its
/// mapping id, how many records it stands for, and where those records disagreed survive
/// round-tripping through the store.
#[derive(Debug, Clone)]
pub struct AssembledFragment {
    /// The interned id of the mapping that produced the fragment.
    pub mapping_id: MappingId,
    /// The source record the fragment was expanded from: for a temporal mapping in a current-state
    /// store, the record observed last.
    pub data: Value,
    /// The records some slot of a temporal mapping kept besides `data`, or `None` when every
    /// attribute comes from `data`. Always `None` for a series observation.
    pub observed: Option<Box<ObservedRecords>>,
    /// How many records the store folded into this fragment: always one for a series observation,
    /// and every record its mapping resolved to the id for a current-state slot.
    pub records: NonZeroU64,
    /// The earliest field on which the records folded into this fragment disagreed, so merging them
    /// discarded a value (see the crate-private `deep_merge`). Always `None`
    /// for a series observation, which is never merged in the store.
    pub conflict: Option<FieldPath>,
    /// The mapping's records the slot released for later ones whose `observedAt` template failed or
    /// whose `observedAt` text reads as no instant (see
    /// [`DiscardedTimestamps`](crate::entity_store::discarded_timestamps::DiscardedTimestamps)).
    /// Always empty for a series observation, which releases nothing.
    pub discarded: DiscardedTimestamps,
}

/// The fragments contributing to one emit-unit.
///
/// A series store puts exactly one observation in each, and a current-state store one fragment per
/// contributing mapping, so the inline capacity of one keeps the overwhelmingly common single-fragment
/// unit off the heap entirely; a series run would otherwise allocate one `Vec` per observation.
pub type StoredUnit = SmallVec<[AssembledFragment; 1]>;

/// Everything the store returns for one base entity id.
///
/// `units` holds one fragment-list per emit-unit: a current-state store returns a single unit holding
/// every mapping's fragment (the join), while a series store returns one unit per observation. An
/// empty `units` means the store held nothing for the id.
#[derive(Debug, Default)]
pub struct AssembledFragments {
    /// The entity's scope, when one was stored.
    pub scope: Option<NgsiLdScope>,
    /// The entity's `expiresAt` (ETSI GS CIM 009 v1.9.1 clause 4.5.1), joined from the expiries its
    /// mappings contribute (see [`join`](crate::entity_store::mapping_expiry::join)).
    pub expires_at: Option<DateTime<Utc>>,
    /// One fragment-list per emit-unit.
    pub units: Vec<StoredUnit>,
}

/// Stores and retrieves entity fragments during resolution.
///
/// Entity stores accumulate source data, scope, and the id of the mapping that produced each fragment
/// under the base entity id, then assemble complete entities. Implementations must be thread-safe
/// (`Send + Sync`): fragments are stored concurrently from many resolver threads.
///
/// Two memory models exist, chosen at construction from the run's temporal target: a current-state
/// store retains one fragment per mapping, a temporal mapping's records observed last or a static
/// mapping's merged records (O(mappings) per id); a series store retains the full append-only series
/// per id.
pub trait EntityStore: Debug + Send + Sync + 'static {
    /// The write pattern this backend is optimized for.
    fn write_strategy(&self) -> StoreWriteStrategy;

    /// Which of a mapping's records for one id this store keeps, which decides whether a record's
    /// observation times are read before it is stored.
    fn retention(&self) -> Retention;

    /// Stores one fragment under its base entity id.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store
    /// write fails.
    fn store_fragment(&self, write: FragmentWrite<'_>) -> Result<()>;

    /// Stores a batch of fragments in a single call.
    ///
    /// Disk-backed implementations coalesce the write into a single transaction, eliminating the
    /// per-fragment read-modify-write overhead of calling [`store_fragment`](Self::store_fragment) in
    /// a loop. The default implementation dispatches per entry.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store
    /// write fails.
    fn store_fragment_batch(&self, fragments: Vec<FragmentWrite<'_>>) -> Result<()> {
        for write in fragments {
            self.store_fragment(write)?;
        }
        Ok(())
    }

    /// Returns the base URNs of all stored entities, one per id.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store
    /// cannot be read.
    fn get_entity_ids(&self) -> Result<Vec<Urn>>;

    /// Iterates over all stored base URNs, calling `callback` with each chunk of ids.
    ///
    /// The default implementation materializes every id first, in the order
    /// [`get_entity_ids`](Self::get_entity_ids) lists them, and moves them into the chunks. A store
    /// that can list its ids without holding them all overrides it to stream chunks directly off a
    /// read transaction.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store
    /// cannot be read, or propagates any error the callback returns.
    fn for_each_entity_id_chunk(&self, chunk_size: usize, callback: &mut dyn FnMut(Vec<Urn>) -> Result<()>) -> Result<()> {
        let mut ids = self.get_entity_ids()?.into_iter();
        loop {
            let chunk: Vec<Urn> = ids.by_ref().take(chunk_size).collect();
            if chunk.is_empty() {
                return Ok(());
            }
            callback(chunk)?;
        }
    }

    /// Assembles the stored fragments for one base id into their emit-units and scope, finishing any
    /// merge of its records with the read sets `mappings` holds.
    ///
    /// **Assembly consumes the id's fragments.** An in-memory store drains them, so calling this
    /// twice for one id yields the fragments once and an empty `units` vec after. That is what the
    /// only caller wants: the scan visits each id exactly once and destroys the store immediately
    /// afterwards, and it is what keeps a series run from deep-copying every stored record on the
    /// way out.
    ///
    /// When no data exists for the id, returns an empty `units` vec; the caller treats that as a
    /// missing entity.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store
    /// cannot be read, a stored fragment cannot be decoded, or a merge to finish names a mapping
    /// `mappings` has no read set for.
    fn assemble_entity(&self, base_id: &Urn, mappings: &dyn MappingReads) -> Result<AssembledFragments>;

    /// Returns the number of distinct base entity ids currently stored.
    fn count(&self) -> usize;

    /// Returns the number of emit-units currently stored across every id.
    ///
    /// For a current-state store this equals [`count`](Self::count) (one unit per id); for a series
    /// store it is the total number of stored observations.
    fn unit_count(&self) -> usize;

    /// Removes all stored data.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store
    /// cannot be cleared.
    fn clear(&self) -> Result<()>;

    /// Releases all resources held by the store.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store's
    /// resources cannot be released.
    fn destroy(&self) -> Result<()>;

    /// Creates a boxed clone of this store.
    fn clone_box(&self) -> Box<dyn EntityStore>;
}

impl Clone for Box<dyn EntityStore> {
    fn clone(&self) -> Box<dyn EntityStore> {
        self.clone_box()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::{
        entity_store::{
            observations::{Observations, RecordRanking},
            record_timestamp::RecordTimestamp,
            store::{EntityStore, FragmentWrite},
        },
        mapping_id::MappingId,
        test_order::{ReadsEverything, next_batch, next_position},
    };
    use cassiopeia_ir::{
        observed_records::{ObservedRecords, RecordChoice},
        source_position::SourcePosition,
    };
    use cassiopeia_mapping::{observed_template::ObservedTemplateIndex, source_reads::SourceReads};
    use cassiopeia_ngsi_ld::entity::scope::{NgsiLdScope, ScopeBuf};
    use chrono::{DateTime, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use urn_rs::Urn;

    /// The read set of a mapping that reads every key, so every disagreement a store test merges
    /// counts.
    pub(crate) static READS_EVERYTHING: SourceReads = SourceReads::Everything;

    /// How a record of a mapping with one `observedAt` template, rendering `observed_at`, ranks.
    pub(crate) fn ranked(observed_at: &str) -> RecordRanking {
        RecordRanking::Ranked(Observations::of([RecordTimestamp::read(json!(observed_at))], &[]))
    }

    /// How a record of a mapping with two `observedAt` templates, each ranking a slot of its own and
    /// rendering `a` and `b`, ranks.
    pub(crate) fn ranked_in_two_slots(a: &str, b: &str) -> RecordRanking {
        RecordRanking::Ranked(Observations::of(
            [RecordTimestamp::read(json!(a)), RecordTimestamp::read(json!(b))],
            &[ObservedTemplateIndex::new(0), ObservedTemplateIndex::new(1)],
        ))
    }

    fn write(base_id: &Urn, mapping: u32, scopes: &[&str]) -> FragmentWrite<'static> {
        FragmentWrite {
            base_id: base_id.clone(),
            source_data: json!({ "mapping": mapping }),
            position: next_position(),
            batch: next_batch(),
            mapping_id: MappingId::new(mapping),
            ranking: RecordRanking::Unranked,
            scope: NgsiLdScope::from_scopes(scopes.iter().map(|scope| ScopeBuf::new(*scope).unwrap())),
            expires_at: None,
            reads: &READS_EVERYTHING,
        }
    }

    /// A write like [`write`], carrying one entity `expiresAt` at `epoch_seconds`, or none when
    /// `epoch_seconds` is `None`.
    fn write_expiring(base_id: &Urn, mapping: u32, epoch_seconds: Option<i64>) -> FragmentWrite<'static> {
        FragmentWrite {
            expires_at: epoch_seconds.map(instant),
            ..write(base_id, mapping, &[])
        }
    }

    /// A write like [`write_expiring`] from a temporal mapping, observed at `observed_at`.
    fn write_observed(base_id: &Urn, mapping: u32, observed_at: &str, epoch_seconds: Option<i64>) -> FragmentWrite<'static> {
        FragmentWrite {
            ranking: ranked(observed_at),
            ..write_expiring(base_id, mapping, epoch_seconds)
        }
    }

    /// A write like [`write_observed`] of the record at `position`, carrying `record`.
    fn write_at(base_id: &Urn, position: SourcePosition, record: Value) -> FragmentWrite<'static> {
        FragmentWrite {
            source_data: record,
            position,
            ..write_observed(base_id, 1, "2026-04-03T22:00:00Z", None)
        }
    }

    /// The position of record `record` of the first input.
    fn at(record: usize) -> SourcePosition {
        SourcePosition::first_of(0).advanced(record)
    }

    fn instant(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().unwrap()
    }

    /// Cross-backend contract every [`EntityStore`] must honour: the scopes every fragment of one id
    /// declares are merged into one sorted set without duplicates, as clause 4.18 of
    /// ETSI GS CIM 009 v1.9.1 mandates when representations of one entity are combined. A fragment
    /// declaring no scope removes none, the merge spans separate writes as well as one batch, and an
    /// id whose fragments declare no scope assembles without one. Every backend calls this so all of
    /// them are held to identical behaviour.
    pub(crate) fn assert_merges_scopes_per_id(store: &dyn EntityStore) {
        let scoped: Urn = "urn:ngsi-ld:Camera:1".parse().unwrap();
        let unscoped: Urn = "urn:ngsi-ld:Camera:2".parse().unwrap();

        store.store_fragment(write(&scoped, 1, &["/d", "/b"])).unwrap();
        store
            .store_fragment_batch(vec![
                write(&scoped, 2, &["/b", "/c"]),
                write(&scoped, 1, &[]),
                write(&scoped, 3, &["/c", "/d"]),
                write(&unscoped, 1, &[]),
            ])
            .unwrap();
        store.store_fragment(write(&scoped, 2, &["/a"])).unwrap();

        let scope = store.assemble_entity(&scoped, &ReadsEverything).unwrap().scope;
        assert_eq!(serde_json::to_value(scope).unwrap(), json!(["/a", "/b", "/c", "/d"]));
        assert_eq!(store.assemble_entity(&unscoped, &ReadsEverything).unwrap().scope, None);
    }

    /// Cross-backend contract every [`EntityStore`] must honour: the expiries the mappings of one id
    /// contribute are joined as clause 4.5.5.3 of ETSI GS CIM 009 v1.9.1 joins the entity-level
    /// `expiresAt` of several versions of one entity. When every mapping declares one the furthest
    /// wins, when any mapping declares none the entity has none, whichever order the mappings
    /// arrive in and across separate writes as well as one batch, and an id whose records declare
    /// no expiry assembles without one. Every backend calls this so all of them are held to
    /// identical behaviour.
    pub(crate) fn assert_joins_expiries_across_mappings(store: &dyn EntityStore) {
        let every_mapping: Urn = "urn:ngsi-ld:Camera:1".parse().unwrap();
        let one_mapping: Urn = "urn:ngsi-ld:Camera:2".parse().unwrap();
        let no_mapping: Urn = "urn:ngsi-ld:Camera:3".parse().unwrap();

        store.store_fragment(write_expiring(&every_mapping, 1, Some(100))).unwrap();
        store.store_fragment(write_expiring(&one_mapping, 1, None)).unwrap();
        store
            .store_fragment_batch(vec![
                write_expiring(&every_mapping, 2, Some(300)),
                write_expiring(&one_mapping, 2, Some(300)),
                write_expiring(&no_mapping, 1, None),
            ])
            .unwrap();
        store.store_fragment(write_expiring(&every_mapping, 3, Some(200))).unwrap();

        assert_eq!(store.assemble_entity(&every_mapping, &ReadsEverything).unwrap().expires_at, Some(instant(300)));
        assert_eq!(store.assemble_entity(&one_mapping, &ReadsEverything).unwrap().expires_at, None);
        assert_eq!(store.assemble_entity(&no_mapping, &ReadsEverything).unwrap().expires_at, None);
    }

    /// Contract every current-state [`EntityStore`] must honour: a mapping's expiry follows the
    /// record the store retains for it, since `expiresAt` says when the stored entity is to be
    /// deleted (ETSI GS CIM 009 v1.9.1 clause 4.22). A temporal record that supersedes the retained
    /// one replaces its expiry, a shorter one and an absent one alike, while a stale record changes
    /// nothing; a static record merged into the retained one overrides its expiry only when it
    /// declares one. Both hold across separate writes as well as within one batch.
    pub(crate) fn assert_expiry_follows_the_retained_record(store: &dyn EntityStore) {
        let shortened: Urn = "urn:ngsi-ld:Camera:1".parse().unwrap();
        let cleared: Urn = "urn:ngsi-ld:Camera:2".parse().unwrap();
        let refined: Urn = "urn:ngsi-ld:Camera:3".parse().unwrap();

        store.store_fragment(write_observed(&shortened, 1, "2026-04-03T22:00:00Z", Some(300))).unwrap();
        store
            .store_fragment_batch(vec![
                write_observed(&shortened, 1, "2026-04-03T22:10:00Z", Some(100)),
                write_observed(&shortened, 1, "2026-04-03T22:05:00Z", Some(500)),
                write_observed(&cleared, 1, "2026-04-03T22:00:00Z", Some(300)),
                write_observed(&cleared, 1, "2026-04-03T22:10:00Z", None),
                write_expiring(&refined, 1, Some(300)),
                write_expiring(&refined, 1, Some(100)),
            ])
            .unwrap();
        store.store_fragment(write_expiring(&refined, 1, None)).unwrap();

        assert_eq!(store.assemble_entity(&shortened, &ReadsEverything).unwrap().expires_at, Some(instant(100)));
        assert_eq!(store.assemble_entity(&cleared, &ReadsEverything).unwrap().expires_at, None);
        assert_eq!(store.assemble_entity(&refined, &ReadsEverything).unwrap().expires_at, Some(instant(100)));
    }

    /// Contract every current-state [`EntityStore`] must honour: a temporal mapping's records rank by
    /// the instant their `observedAt` names, not by its text (ETSI GS CIM 009 v1.9.1 clause 4.5.5.3
    /// keeps the most recent `observedAt` `DateTime`). An offset is applied before instants compare, an
    /// epoch ranks by the instant it names however many digits it has, text that reads as no instant
    /// ranks below one that does, and a day-first date ranks by its day. Each pair is written in
    /// source order across separate writes, and in reverse within one batch, and keeps the same
    /// record either way.
    pub(crate) fn assert_ranks_by_instant(store: &dyn EntityStore) {
        let stations = [
            ("S1", [("2026-03-01T11:00:00Z", 20.0), ("2026-03-01T12:00:00+02:00", 25.0)], 20.0),
            ("S2", [("999999999", 7.0), ("1000000000", 8.0)], 8.0),
            ("S3", [("2026-03-01T11:00:00Z", 30.0), ("soon", 31.0)], 30.0),
            ("S4", [("02/03/2026", 40.0), ("01/04/2026", 41.0)], 41.0),
        ];
        let observation = |id: &Urn, record: usize, (time, t): (&str, f64)| FragmentWrite {
            ranking: ranked(time),
            ..write_at(id, at(record), json!({ "t": t }))
        };
        for (station, [first, second], kept) in stations {
            let in_order: Urn = format!("urn:ngsi-ld:WeatherObserved:{station}-in-order").parse().unwrap();
            let reversed: Urn = format!("urn:ngsi-ld:WeatherObserved:{station}-reversed").parse().unwrap();

            store.store_fragment(observation(&in_order, 0, first)).unwrap();
            store.store_fragment(observation(&in_order, 1, second)).unwrap();
            store
                .store_fragment_batch(vec![observation(&reversed, 1, second), observation(&reversed, 0, first)])
                .unwrap();

            for id in [in_order, reversed] {
                let units = store.assemble_entity(&id, &ReadsEverything).unwrap().units;
                assert_eq!(units[0][0].data, json!({ "t": kept }), "{id}");
            }
        }
    }

    /// Contract every current-state [`EntityStore`] must honour: a mapping whose attributes carry
    /// distinct `observedAt` templates ranks its records in a slot per template (ETSI GS CIM 009
    /// v1.9.1 clause 4.5.5.3 resolves conflicting instances per attribute). Of a record newer in the
    /// first slot and a record newer in the second and over all, the fragment holds the second as the
    /// record observed last and the first beside it for the first slot, whether the records arrive in
    /// source order across separate writes or in reverse within one batch.
    pub(crate) fn assert_ranks_each_slot_on_its_own(store: &dyn EntityStore) {
        let written: Urn = "urn:ngsi-ld:Sensor:written".parse().unwrap();
        let batched: Urn = "urn:ngsi-ld:Sensor:batched".parse().unwrap();
        let first = |id: &Urn| FragmentWrite {
            ranking: ranked_in_two_slots("2026-03-01T11:00:00Z", "2026-03-01T09:00:00Z"),
            ..write_at(id, at(0), json!({ "record": 1 }))
        };
        let second = |id: &Urn| FragmentWrite {
            ranking: ranked_in_two_slots("2026-03-01T10:00:00Z", "2026-03-01T12:00:00Z"),
            ..write_at(id, at(1), json!({ "record": 2 }))
        };

        store.store_fragment(first(&written)).unwrap();
        store.store_fragment(second(&written)).unwrap();
        store.store_fragment_batch(vec![second(&batched), first(&batched)]).unwrap();

        for id in [written, batched] {
            let units = store.assemble_entity(&id, &ReadsEverything).unwrap().units;
            assert_eq!(units[0][0].data, json!({ "record": 2 }), "{id}");
            assert_eq!(
                units[0][0].observed.as_deref(),
                Some(&ObservedRecords::new(
                    vec![json!({ "record": 1 })],
                    vec![RecordChoice::Observed(0), RecordChoice::Newest]
                )),
                "{id}"
            );
        }
    }

    /// Contract every series [`EntityStore`] must honour: a mapping's observations contribute the
    /// expiry furthest in the future, an observation declaring none clearing nothing, since the
    /// stored series is due for deletion only once its last observation is (ETSI GS CIM 009 v1.9.1
    /// clause 4.22). This holds across separate writes as well as within one batch.
    pub(crate) fn assert_series_keeps_the_furthest_expiry_per_mapping(store: &dyn EntityStore) {
        let id: Urn = "urn:ngsi-ld:Camera:1".parse().unwrap();

        store.store_fragment(write_observed(&id, 1, "2026-04-03T22:00:00Z", Some(100))).unwrap();
        store
            .store_fragment_batch(vec![
                write_observed(&id, 1, "2026-04-03T22:05:00Z", Some(300)),
                write_observed(&id, 1, "2026-04-03T22:10:00Z", None),
            ])
            .unwrap();
        store.store_fragment(write_observed(&id, 1, "2026-04-03T22:15:00Z", Some(200))).unwrap();

        assert_eq!(store.assemble_entity(&id, &ReadsEverything).unwrap().expires_at, Some(instant(300)));
    }

    /// Contract every series [`EntityStore`] must honour: an id's observations are handed on in the
    /// order of their records in the source, whatever order they reached the store in, across
    /// separate writes as well as within one batch, and the records of a later input after every
    /// record of an earlier one.
    pub(crate) fn assert_series_hands_observations_on_in_source_order(store: &dyn EntityStore) {
        let id: Urn = "urn:ngsi-ld:Camera:1".parse().unwrap();

        store.store_fragment(write_at(&id, at(3), json!(3))).unwrap();
        store.store_fragment(write_at(&id, SourcePosition::first_of(1), json!("second input"))).unwrap();
        store
            .store_fragment_batch(vec![
                write_at(&id, at(5), json!(5)),
                write_at(&id, at(1), json!(1)),
                write_at(&id, at(4), json!(4)),
            ])
            .unwrap();
        store.store_fragment(write_at(&id, at(2), json!(2))).unwrap();

        let records: Vec<Value> = store
            .assemble_entity(&id, &ReadsEverything)
            .unwrap()
            .units
            .into_iter()
            .map(|unit| unit[0].data.clone())
            .collect();
        assert_eq!(records, [json!(1), json!(2), json!(3), json!(4), json!(5), json!("second input")]);
    }

    /// Contract every [`EntityStore`] must honour: ids are handed on in the order of their first
    /// records in the source, however their records reached the store. A record that comes before
    /// every record of its id stored so far moves the id earlier, whichever of the id's mappings it
    /// comes from, a later record never moves it, and ids whose first record is one record follow the
    /// bytewise order of their text. Listing the ids and walking them chunk by chunk agree, and the
    /// count is of distinct ids.
    pub(crate) fn assert_hands_ids_on_in_order_of_their_first_records(store: &dyn EntityStore) {
        let [late, early, moved, shared_b, shared_a, joined] =
            ["Late", "Early", "Moved", "Shared-B", "Shared-A", "Joined"].map(|name| format!("urn:ngsi-ld:Camera:{name}").parse::<Urn>().unwrap());

        store.store_fragment(write_at(&late, at(9), json!(9))).unwrap();
        store
            .store_fragment_batch(vec![
                write_at(&early, at(2), json!(2)),
                write_at(&moved, at(7), json!(7)),
                write_at(&shared_b, at(4), json!(4)),
                write_at(&shared_a, at(4), json!(4)),
                write_at(&early, at(1), json!(1)),
                write_at(&joined, at(8), json!(8)),
            ])
            .unwrap();
        store.store_fragment(write_at(&moved, at(0), json!(0))).unwrap();
        store.store_fragment(write_at(&late, at(3), json!(3))).unwrap();
        store
            .store_fragment(FragmentWrite {
                mapping_id: MappingId::new(2),
                ..write_at(&joined, at(5), json!(5))
            })
            .unwrap();
        store
            .store_fragment_batch(vec![write_at(&early, at(6), json!(6)), write_at(&moved, at(10), json!(10))])
            .unwrap();

        let expected = [moved, early, late, shared_a, shared_b, joined];
        assert_eq!(store.get_entity_ids().unwrap(), expected);
        let mut walked = Vec::new();
        store
            .for_each_entity_id_chunk(2, &mut |chunk| {
                walked.extend(chunk);
                Ok(())
            })
            .unwrap();
        assert_eq!(walked, expected);
        assert_eq!(store.count(), 6);
    }
}
