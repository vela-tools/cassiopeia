use crate::entity_store::record_failures::RecordFailures;
use cassiopeia_mapping::{observed_at::ObservedAt, observed_template::ObservedTemplateIndex};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

/// The records a current-state slot released without an instant to rank them by, for a loss the
/// run reports.
///
/// The records a slot keeps reach the extractor, which reports a failed `observedAt` template or
/// unreadable `observedAt` text itself; a record the slot releases never does, so the slot counts
/// its losses as it releases it, and each loss is reported exactly once whichever record wins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscardedTimestamps {
    /// How many records released had an `observedAt` template fail.
    unresolvable: u64,
    /// The records released whose `observedAt` text read as no instant, one entry per template, in
    /// the order the templates first lost one.
    unreadable: Vec<UnreadableDiscards>,
}

/// The records a slot released whose text under one `observedAt` template read as no instant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnreadableDiscards {
    /// The template that rendered the text.
    pub template: ObservedTemplateIndex,
    /// The first text that read as no instant. It illustrates the loss, it does not identify every
    /// record that shared it.
    pub example: ObservedAt,
    /// How many records were released with such text.
    pub records: NonZeroU64,
}

impl DiscardedTimestamps {
    /// Counts the losses of a record released with `failures`.
    ///
    /// A record counts once among those whose template failed however many of its templates did,
    /// and once under each template that rendered unreadable text for it.
    pub fn discard(&mut self, failures: RecordFailures) {
        if failures.any_unresolvable() {
            self.unresolvable = self.unresolvable.saturating_add(1);
        }
        for (template, text) in failures.into_unreadable() {
            match self.unreadable.iter_mut().find(|unreadable| unreadable.template == template) {
                Some(unreadable) => unreadable.records = unreadable.records.saturating_add(1),
                None => self.unreadable.push(UnreadableDiscards {
                    template,
                    example: text,
                    records: NonZeroU64::MIN,
                }),
            }
        }
    }

    /// How many records released had an `observedAt` template fail.
    #[must_use]
    pub const fn unresolvable(&self) -> u64 {
        self.unresolvable
    }

    /// The records released whose `observedAt` text read as no instant, per template.
    #[must_use]
    pub fn unreadable(&self) -> &[UnreadableDiscards] {
        &self.unreadable
    }

    /// Whether no record was released for a loss the run reports.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.unresolvable == 0 && self.unreadable.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_store::{
        discarded_timestamps::{DiscardedTimestamps, UnreadableDiscards},
        record_failures::RecordFailures,
        record_timestamp::RecordTimestamp,
    };
    use cassiopeia_mapping::{observed_at::ObservedAt, observed_template::ObservedTemplateIndex};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU64;

    fn failures(timestamps: impl IntoIterator<Item = RecordTimestamp>) -> RecordFailures {
        let mut failures = RecordFailures::default();
        for (index, timestamp) in timestamps.into_iter().enumerate() {
            failures.note(ObservedTemplateIndex::new(index), timestamp);
        }
        failures
    }

    fn unreadable(text: &str) -> RecordTimestamp {
        RecordTimestamp::Unreadable(ObservedAt::new(text))
    }

    #[test]
    fn a_record_without_losses_is_no_discard() {
        let mut discarded = DiscardedTimestamps::default();
        discarded.discard(failures([RecordTimestamp::Absent]));

        assert!(discarded.is_empty());
    }

    #[test]
    fn a_record_counts_once_among_the_failed_however_many_of_its_templates_failed() {
        let mut discarded = DiscardedTimestamps::default();
        discarded.discard(failures([RecordTimestamp::Unresolvable, RecordTimestamp::Unresolvable]));
        discarded.discard(failures([RecordTimestamp::Unresolvable]));

        assert_eq!((discarded.unresolvable(), discarded.unreadable()), (2, &[][..]));
    }

    #[test]
    fn unreadable_text_is_counted_per_template_keeping_each_template_s_first_text() {
        let mut discarded = DiscardedTimestamps::default();
        discarded.discard(failures([unreadable("soon"), RecordTimestamp::Absent]));
        discarded.discard(failures([unreadable("later"), unreadable("never")]));

        assert_eq!(
            discarded.unreadable(),
            [
                UnreadableDiscards {
                    template: ObservedTemplateIndex::new(0),
                    example: ObservedAt::new("soon"),
                    records: NonZeroU64::new(2).unwrap(),
                },
                UnreadableDiscards {
                    template: ObservedTemplateIndex::new(1),
                    example: ObservedAt::new("never"),
                    records: NonZeroU64::MIN,
                },
            ]
        );
        assert_eq!(discarded.unresolvable(), 0);
    }

    #[test]
    fn discards_survive_the_store_encoding() {
        let mut discarded = DiscardedTimestamps::default();
        discarded.discard(failures([RecordTimestamp::Unresolvable, unreadable("soon")]));
        let encoded = rmp_serde::to_vec(&discarded).unwrap();

        assert_eq!(rmp_serde::from_slice::<DiscardedTimestamps>(&encoded).unwrap(), discarded);
    }
}
