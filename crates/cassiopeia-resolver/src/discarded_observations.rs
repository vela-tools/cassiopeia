use crate::{mapping_id::MappingId, observed_at_failures::ObservedAtFailures};
use ahash::AHashMap;
use cassiopeia_mapping::template::error::TemplateError;
use std::num::NonZeroU64;
use urn_rs::Urn;

/// What one mapping's failing `observedAt` template cost across a scan: the records a current-state
/// store discarded because they ranked below another record of the same id.
///
/// Only the first failure and the first id are kept, as for any template failure: the template is
/// the problem, the id is one place to go and look, and the counts say how far it spread.
#[derive(Debug)]
pub struct DiscardedObservation {
    /// The template's first failure, kept whole so a report can walk its cause chain.
    pub error: TemplateError,
    /// The first entity id a discarded record resolved to.
    pub entity: Urn,
    /// How many ids lost records this way.
    pub entities: NonZeroU64,
    /// How many records were discarded, across every id.
    pub records: NonZeroU64,
}

/// The records an assembly scan found discarded for a failing `observedAt` template, one entry per
/// mapping.
#[derive(Debug, Default)]
pub struct DiscardedObservations {
    by_mapping: AHashMap<MappingId, DiscardedObservation>,
}

impl DiscardedObservations {
    /// Opens an empty tally.
    #[must_use]
    pub fn new() -> DiscardedObservations {
        DiscardedObservations::default()
    }

    /// Records that `records` records of the mapping `mapping_id` resolving to `entity` were
    /// discarded, taking the mapping's failure out of `failures` the first time it is met.
    ///
    /// A store counts a discard only for a record it was told the template failed on, and the
    /// failure is recorded in the same step, so a mapping with discards always has a failure to take.
    pub fn record(&mut self, mapping_id: MappingId, entity: &Urn, records: NonZeroU64, failures: &ObservedAtFailures) {
        if let Some(discarded) = self.by_mapping.get_mut(&mapping_id) {
            discarded.entities = discarded.entities.saturating_add(1);
            discarded.records = discarded.records.saturating_add(records.get());
            return;
        }
        if let Some(error) = failures.take(mapping_id) {
            self.by_mapping.insert(
                mapping_id,
                DiscardedObservation {
                    error,
                    // The tally outlives the borrowed id the assembly scan holds.
                    entity: entity.clone(),
                    entities: NonZeroU64::MIN,
                    records,
                },
            );
        }
    }

    /// Whether the scan found no discarded records at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_mapping.is_empty()
    }

    /// Every mapping's discards, ordered by mapping id so a report lists them the same way every run.
    #[must_use]
    pub fn into_entries(self) -> Vec<DiscardedObservation> {
        let mut entries: Vec<(MappingId, DiscardedObservation)> = self.by_mapping.into_iter().collect();
        entries.sort_by_key(|(mapping_id, _)| *mapping_id);
        entries.into_iter().map(|(_, discarded)| discarded).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        discarded_observations::DiscardedObservations,
        mapping_id::MappingId,
        observed_at_failures::{ObservedAtFailures, tests::failure},
    };
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU64;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    fn count(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    #[test]
    fn a_fresh_tally_holds_nothing() {
        assert!(DiscardedObservations::new().is_empty());
        assert!(DiscardedObservations::new().into_entries().is_empty());
    }

    #[test]
    fn one_mapping_keeps_its_first_id_and_sums_ids_and_records() {
        let failures = ObservedAtFailures::new();
        failures.record(MappingId::new(1), failure("{{ ts | trim }}"));
        let mut tally = DiscardedObservations::new();

        tally.record(MappingId::new(1), &urn("urn:ngsi-ld:Thing:A"), count(2), &failures);
        tally.record(MappingId::new(1), &urn("urn:ngsi-ld:Thing:B"), count(3), &failures);

        let entries = tally.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entity, urn("urn:ngsi-ld:Thing:A"));
        assert_eq!(entries[0].entities.get(), 2);
        assert_eq!(entries[0].records.get(), 5);
    }

    #[test]
    fn each_mapping_is_its_own_entry_in_mapping_order() {
        let failures = ObservedAtFailures::new();
        failures.record(MappingId::new(1), failure("{{ a }}"));
        failures.record(MappingId::new(2), failure("{{ b }}"));
        let mut tally = DiscardedObservations::new();

        tally.record(MappingId::new(2), &urn("urn:ngsi-ld:Thing:B"), count(1), &failures);
        tally.record(MappingId::new(1), &urn("urn:ngsi-ld:Thing:A"), count(1), &failures);

        let entities: Vec<Urn> = tally.into_entries().into_iter().map(|discarded| discarded.entity).collect();
        assert_eq!(entities, [urn("urn:ngsi-ld:Thing:A"), urn("urn:ngsi-ld:Thing:B")]);
    }
}
