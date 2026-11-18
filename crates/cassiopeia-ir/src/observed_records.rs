use cassiopeia_mapping::observation_slot::SlotIndex;
use serde_json::Value as JsonValue;

/// Which of a fragment's records one slot of its mapping kept: the newest record the fragment
/// carries, or one of the others it was observed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordChoice {
    /// The fragment's own record, the one observed last over every template of the mapping.
    Newest,
    /// The record at this position among [`ObservedRecords`]'s.
    Observed(usize),
}

/// The records a current-state fragment carries besides its newest one, for the slots that kept
/// another.
///
/// A mapping whose attributes carry distinct `observedAt` templates ranks each template in a slot of
/// its own, so one attribute can come from a different record than another (ETSI GS CIM 009 v1.9.1
/// clause 4.5.5.3). Each record is held once, however many slots kept it.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedRecords {
    /// The records some slot kept that are not the newest one, each once.
    records: Vec<JsonValue>,
    /// The record each slot kept, slot by slot.
    by_slot: Vec<RecordChoice>,
}

impl ObservedRecords {
    /// The records `records`, chosen by each slot as `by_slot` says.
    #[must_use]
    pub const fn new(records: Vec<JsonValue>, by_slot: Vec<RecordChoice>) -> ObservedRecords {
        ObservedRecords { records, by_slot }
    }

    /// The record `slot` kept; the newest for a slot the records do not know.
    #[must_use]
    pub fn choice(&self, slot: SlotIndex) -> RecordChoice {
        self.by_slot.get(slot.get()).copied().unwrap_or(RecordChoice::Newest)
    }

    /// The record `choice` names, `None` for the newest one, which the fragment holds itself, or for
    /// a position past the records held.
    #[must_use]
    pub fn record(&self, choice: RecordChoice) -> Option<&JsonValue> {
        match choice {
            RecordChoice::Newest => None,
            RecordChoice::Observed(index) => self.records.get(index),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::observed_records::{ObservedRecords, RecordChoice};
    use cassiopeia_mapping::observation_slot::SlotIndex;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn each_slot_names_the_record_it_kept() {
        let observed = ObservedRecords::new(vec![json!({"a": 1})], vec![RecordChoice::Observed(0), RecordChoice::Newest]);

        assert_eq!(observed.choice(SlotIndex::new(0)), RecordChoice::Observed(0));
        assert_eq!(observed.record(observed.choice(SlotIndex::new(0))), Some(&json!({"a": 1})));
        assert_eq!(observed.choice(SlotIndex::new(1)), RecordChoice::Newest);
        assert_eq!(observed.record(RecordChoice::Newest), None);
    }

    #[test]
    fn a_slot_or_record_the_records_do_not_know_falls_back_to_the_newest() {
        let observed = ObservedRecords::new(Vec::new(), Vec::new());

        assert_eq!(observed.choice(SlotIndex::new(3)), RecordChoice::Newest);
        assert_eq!(observed.record(RecordChoice::Observed(0)), None);
    }
}
