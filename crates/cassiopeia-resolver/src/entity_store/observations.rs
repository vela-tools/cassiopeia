use crate::entity_store::{observation_instant::ObservationInstant, record_failures::RecordFailures, record_timestamp::RecordTimestamp};
use cassiopeia_mapping::observed_template::ObservedTemplateIndex;
use smallvec::SmallVec;

/// How a record of a temporal mapping ranks in a current-state store: by the latest instant over
/// every `observedAt` template its mapping declares, and in each slot by the instant its slot's
/// template names.
///
/// A record takes part in every slot whatever values it holds, so a newer record without a value
/// still wins the slot, and the attribute is absent, as it would be from a record kept whole.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observations {
    /// The latest instant any of the record's templates names, which ranks it as the record observed
    /// last.
    latest: Option<ObservationInstant>,
    /// The instant each slot's template names, slot by slot. Empty for a mapping without slots, so
    /// the common single-template record holds nothing on the heap.
    slots: Vec<Option<ObservationInstant>>,
    /// The losses the record's templates met.
    failures: RecordFailures,
}

impl Observations {
    /// What a record whose templates yielded `timestamps`, one per template in the order of the
    /// mapping's [`ObservationPlan`](cassiopeia_mapping::observation_plan::ObservationPlan), ranks by,
    /// in slots ranking by `slot_templates`.
    #[must_use]
    pub fn of(timestamps: impl IntoIterator<Item = RecordTimestamp>, slot_templates: &[ObservedTemplateIndex]) -> Observations {
        let mut failures = RecordFailures::default();
        let instants: SmallVec<[Option<ObservationInstant>; 2]> = timestamps
            .into_iter()
            .enumerate()
            .map(|(index, timestamp)| {
                let instant = timestamp.instant();
                failures.note(ObservedTemplateIndex::new(index), timestamp);
                instant
            })
            .collect();
        Observations {
            latest: instants.iter().copied().max().flatten(),
            slots: slot_templates.iter().map(|template| instants.get(template.get()).copied().flatten()).collect(),
            failures,
        }
    }

    /// The parts a store ranks the record by: its latest instant, each slot's instant, and its
    /// losses.
    #[must_use]
    pub fn into_parts(self) -> (Option<ObservationInstant>, Vec<Option<ObservationInstant>>, RecordFailures) {
        (self.latest, self.slots, self.failures)
    }
}

/// How a record a store is handed ranks against the other records of its mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordRanking {
    /// The record carries nothing to rank by: its mapping declares no `observedAt`, so a
    /// current-state store merges it with the mapping's other records, or the store keeps every
    /// record as a series does.
    Unranked,
    /// The record ranks by these observations.
    Ranked(Observations),
}

impl RecordRanking {
    /// The observations the record ranks by; none for a record that carries nothing to rank by,
    /// which ranks below every observed one.
    #[must_use]
    pub fn into_observations(self) -> Observations {
        match self {
            RecordRanking::Ranked(observations) => observations,
            RecordRanking::Unranked => Observations::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_store::{
        observation_instant::ObservationInstant,
        observations::{Observations, RecordRanking},
        record_failures::RecordFailures,
        record_timestamp::RecordTimestamp,
    };
    use cassiopeia_mapping::{observed_at::ObservedAt, observed_template::ObservedTemplateIndex};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn instant(text: &str) -> ObservationInstant {
        ObservationInstant::from(text.parse::<DateTime<Utc>>().unwrap())
    }

    fn timestamp(text: &str) -> RecordTimestamp {
        RecordTimestamp::read(json!(text))
    }

    #[test]
    fn one_template_ranks_the_record_by_its_instant_in_no_slot() {
        let observations = Observations::of([timestamp("2026-03-01T11:00:00Z")], &[]);

        assert_eq!(
            observations.into_parts(),
            (Some(instant("2026-03-01T11:00:00Z")), Vec::new(), RecordFailures::default())
        );
    }

    #[test]
    fn the_latest_template_ranks_the_record_and_each_slot_ranks_by_its_own() {
        let observations = Observations::of(
            [timestamp("2026-03-01T09:00:00Z"), timestamp("soon"), timestamp("2026-03-01T12:00:00Z")],
            &[ObservedTemplateIndex::new(2), ObservedTemplateIndex::new(1), ObservedTemplateIndex::new(0)],
        );

        let (latest, slots, failures) = observations.into_parts();
        assert_eq!(latest, Some(instant("2026-03-01T12:00:00Z")));
        assert_eq!(slots, [Some(instant("2026-03-01T12:00:00Z")), None, Some(instant("2026-03-01T09:00:00Z"))]);
        assert_eq!(
            failures.into_unreadable().collect::<Vec<_>>(),
            [(ObservedTemplateIndex::new(1), ObservedAt::new("soon"))]
        );
    }

    #[test]
    fn a_record_without_any_instant_ranks_by_none() {
        let observations = Observations::of([RecordTimestamp::Absent, RecordTimestamp::Unresolvable], &[ObservedTemplateIndex::new(0)]);

        let (latest, slots, failures) = observations.into_parts();
        assert_eq!((latest, slots), (None, vec![None]));
        assert!(failures.any_unresolvable());
    }

    #[test]
    fn an_unranked_record_ranks_as_observed_at_no_instant() {
        assert_eq!(RecordRanking::Unranked.into_observations(), Observations::default());
        let ranked = Observations::of([timestamp("2026-03-01T11:00:00Z")], &[]);
        assert_eq!(RecordRanking::Ranked(ranked.clone()).into_observations(), ranked);
    }
}
