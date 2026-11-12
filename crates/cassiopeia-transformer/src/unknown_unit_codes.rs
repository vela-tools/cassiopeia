use crate::unknown_unit_code::{UnknownUnitCode, UnknownUnitCodeRef};
use cassiopeia_batch_sink::{batch_sink::BatchSink, occurrences::Occurrences};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use derive_more::{Deref, Into};

/// The `unitCode` texts one batch could not write, one count per attribute and text.
///
/// ETSI GS CIM 009 v1.9.1 clause 4.5.2 admits only a UN/CEFACT Recommendation No. 20 common code as
/// a `unitCode`, so an attribute given any other text is emitted without the qualifier. The reading
/// is kept, and the run names the loss once per attribute and text, with how many records shared it.
#[derive(Default, Deref, Into)]
pub struct UnknownUnitCodes(BatchSink<UnknownUnitCode, Occurrences>);

impl UnknownUnitCodes {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> UnknownUnitCodes {
        UnknownUnitCodes::default()
    }

    /// Records that `attribute` was given `text` as its `unitCode`, which is no Recommendation No. 20
    /// code.
    ///
    /// The first record of an attribute and text opens its entry; every later one is counted against
    /// that same entry.
    pub fn record(&self, attribute: &NameBuf, text: &str) {
        let key = UnknownUnitCodeRef { attribute, text };
        self.0.tally(&key, || (key.to_owned_key(), Occurrences::first()));
    }
}

#[cfg(test)]
mod tests {
    use crate::unknown_unit_codes::UnknownUnitCodes;
    use cassiopeia_batch_sink::{batch_sink::BatchSink, recurring::Recurring};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    /// Every entry of `sink` as its attribute, text, and count, in the order the entries were opened.
    fn entries(sink: UnknownUnitCodes) -> Vec<(String, String, u64)> {
        BatchSink::from(sink)
            .into_entries()
            .map(|(key, record)| (key.attribute.to_string(), key.text.into_string(), record.occurrences().get()))
            .collect()
    }

    fn entry(attribute: &str, text: &str, occurrences: u64) -> (String, String, u64) {
        (attribute.to_owned(), text.to_owned(), occurrences)
    }

    #[test]
    fn a_fresh_sink_holds_nothing() {
        assert!(UnknownUnitCodes::new().is_empty());
    }

    #[test]
    fn entries_keep_the_order_they_were_first_met_in() {
        let sink = UnknownUnitCodes::new();
        sink.record(&name("pm10"), "x");
        sink.record(&name("no2"), "y");
        sink.record(&name("pm10"), "x");

        assert_eq!(entries(sink), [entry("pm10", "x", 2), entry("no2", "y", 1)]);
    }

    #[test]
    fn one_attribute_with_two_texts_is_two_entries() {
        let sink = UnknownUnitCodes::new();
        sink.record(&name("pm10"), "x");
        sink.record(&name("pm10"), "y");

        assert_eq!(entries(sink), [entry("pm10", "x", 1), entry("pm10", "y", 1)]);
    }
}
