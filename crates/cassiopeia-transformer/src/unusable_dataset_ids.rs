use cassiopeia_batch_sink::{batch_sink::BatchSink, counted_example::CountedExample};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use derive_more::{Deref, Into};

/// The `datasetId` texts one batch could not write, one record per attribute.
///
/// ETSI GS CIM 009 v1.9.1 clause 4.5.5.1 admits only a URI, or `@none`, as a `datasetId`. An
/// instance whose `datasetId` is any other text cannot be written as the instance the mapping
/// declared: written without it, it would become the attribute's default instance. The instance, or
/// the single attribute, is dropped instead, and the run names the loss once per attribute, with the
/// first text it met and how many declarations it dropped.
#[derive(Default, Deref, Into)]
pub struct UnusableDatasetIds(BatchSink<NameBuf, CountedExample>);

impl UnusableDatasetIds {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> UnusableDatasetIds {
        UnusableDatasetIds::default()
    }

    /// Records that a declaration of `attribute` was given `text` as its `datasetId`, which is no
    /// absolute URI, and was dropped.
    ///
    /// The first text an attribute fails on is kept as its example; every later one is counted
    /// against that same record.
    pub fn record(&self, attribute: &NameBuf, text: &str) {
        // The sink owns the keys it files, so the name is copied once, when the attribute's first
        // loss opens its record; every later loss is looked up by reference.
        self.0.tally(attribute, || (attribute.clone(), CountedExample::first(text)));
    }
}

#[cfg(test)]
mod tests {
    use crate::unusable_dataset_ids::UnusableDatasetIds;
    use cassiopeia_batch_sink::batch_sink::BatchSink;
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    /// Every entry of `sink` as its attribute, example text, and count, in the order the entries
    /// were opened.
    fn entries(sink: UnusableDatasetIds) -> Vec<(String, String, u64)> {
        BatchSink::from(sink)
            .into_entries()
            .map(|(attribute, record)| (attribute.to_string(), record.example.into_string(), record.occurrences.get()))
            .collect()
    }

    fn entry(attribute: &str, example: &str, occurrences: u64) -> (String, String, u64) {
        (attribute.to_owned(), example.to_owned(), occurrences)
    }

    #[test]
    fn a_fresh_sink_holds_nothing() {
        assert!(UnusableDatasetIds::new().is_empty());
    }

    #[test]
    fn two_texts_for_one_attribute_are_one_entry_keeping_the_first_text() {
        let sink = UnusableDatasetIds::new();
        sink.record(&name("temperature"), "gfs model");
        sink.record(&name("temperature"), "datasets/gfs");

        assert_eq!(entries(sink), [entry("temperature", "gfs model", 2)]);
    }

    #[test]
    fn two_attributes_are_two_entries_in_the_order_they_failed() {
        let sink = UnusableDatasetIds::new();
        sink.record(&name("temperature"), "gfs model");
        sink.record(&name("humidity"), "x y");

        assert_eq!(entries(sink), [entry("temperature", "gfs model", 1), entry("humidity", "x y", 1)]);
    }
}
