use crate::{attribute_path::AttributePath, required_value::RequiredValue, unusable_value::UnusableValue};
use cassiopeia_batch_sink::{batch_sink::BatchSink, counted_example::CountedExample};
use derive_more::{Deref, Into};

/// The values one batch dropped because their attribute's kind cannot hold them, one record per
/// [`UnusableValue`].
#[derive(Default, Deref, Into)]
pub struct UnusableValues(BatchSink<UnusableValue, CountedExample>);

impl UnusableValues {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> UnusableValues {
        UnusableValues::default()
    }

    /// Records that the declaration at `attribute` carried `text`, which is not the `required`
    /// value.
    ///
    /// The first text a declaration fails on is kept as its example; every later one is counted
    /// against that same record.
    pub fn record(&self, attribute: AttributePath, required: RequiredValue, text: &str) {
        self.0.tally_owned(UnusableValue { attribute, required }, || CountedExample::first(text));
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_path::AttributePath, required_value::RequiredValue, unusable_value::UnusableValue, unusable_values::UnusableValues};
    use cassiopeia_batch_sink::{batch_sink::BatchSink, counted_example::CountedExample, recurring::Recurring};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;

    fn path(root: &str, nested: &[&str]) -> AttributePath {
        let name = |value: &str| NameBuf::new(value).expect("valid name");
        AttributePath::new(name(root), nested.iter().copied().map(name).collect(), Vec::new())
    }

    fn entries(sink: UnusableValues) -> Vec<(UnusableValue, CountedExample)> {
        BatchSink::from(sink).into_entries().collect()
    }

    #[test]
    fn a_fresh_sink_holds_nothing() {
        assert!(UnusableValues::new().is_empty());
    }

    #[test]
    fn every_value_one_declaration_fails_on_is_counted_against_one_record_keeping_the_first() {
        let sink = UnusableValues::new();
        sink.record(path("category", &[]), RequiredValue::Iri, "not an iri");
        sink.record(path("category", &[]), RequiredValue::Iri, "also not one");

        let entries = entries(sink);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.attribute, path("category", &[]));
        assert_eq!(entries[0].1.example.as_ref(), "not an iri");
        assert_eq!(entries[0].1.occurrences().get(), 2);
    }

    #[test]
    fn a_sub_attribute_is_a_record_apart_from_a_top_level_attribute_of_the_same_name() {
        let sink = UnusableValues::new();
        sink.record(path("level", &[]), RequiredValue::Iri, "high");
        sink.record(path("temperature", &["level"]), RequiredValue::Iri, "low");

        let entries = entries(sink);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0.attribute, path("level", &[]));
        assert_eq!(entries[1].0.attribute, path("temperature", &["level"]));
    }

    #[test]
    fn one_path_under_two_requirements_is_two_records() {
        let sink = UnusableValues::new();
        sink.record(path("label", &[]), RequiredValue::Iri, "one");
        sink.record(path("label", &[]), RequiredValue::LanguageMap, "two");

        assert_eq!(entries(sink).len(), 2);
    }
}
