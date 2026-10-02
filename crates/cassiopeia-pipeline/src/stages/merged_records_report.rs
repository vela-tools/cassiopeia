use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, resolver_code::ResolverCode},
    context_field::{ContextField, minted_id},
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::DiagnosticSink;
use cassiopeia_resolver::{
    merged_records::{MergedRecords, MergedType},
    record_merge::RecordMerge,
};

/// The one-line summary: how many records landed on the exemplar id, the earliest source field they
/// disagreed on, that values were discarded, and how many further ids of the type disagreed too.
///
/// The headline carries the id, the count, and the field because the concise form shows nothing else:
/// the entities travel as a context field, which is only rendered under `-v`, and a reader who cannot
/// see which id lost which field has nothing to go and inspect. It does not say which record's value
/// was kept: with records arriving in parallel that is not the same from one run to the next.
fn headline(merged: &MergedType) -> String {
    let exemplar = &merged.exemplar;
    let first = format!(
        "{} records resolved to `{}` and disagreed on `{}`; one value of each disagreeing field was kept and the others were discarded",
        exemplar.records, exemplar.id, exemplar.field
    );
    match merged.ids.get() - 1 {
        0 => first,
        1 => format!("{first}; the records of 1 more `{}` entity disagreed too", exemplar.entity_type),
        more => format!("{first}; the records of {more} more `{}` entities disagreed too", exemplar.entity_type),
    }
}

/// Names each entity type whose records disagreed, once, and returns how many ids they disagreed on.
///
/// One diagnostic per type, however many of its ids lost values: the type and the code are the
/// failure, the exemplar id, its record count, and the field ride on the headline, and every other id
/// is counted as an occurrence, so a source that repeats ten thousand ids still prints one line per
/// type. Each such id is one warning, the same unit the stage's warning total counts in.
pub(crate) fn report_merged_records(sink: &dyn DiagnosticSink, merged: MergedRecords) -> u64 {
    if merged.is_empty() {
        return 0;
    }

    let mut total: u64 = 0;
    for entry in merged.into_entries() {
        let headline = headline(&entry);
        let MergedType {
            exemplar: RecordMerge { id, entity_type, .. },
            ids,
        } = entry;
        let entities = minted_id(&id).map(|first| ContextField::Entities {
            first,
            additional: ids.get() - 1,
        });
        sink.report(
            &DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Resolver(ResolverCode::RecordsMerged), headline)
                .with_context(ContextField::EntityType(entity_type))
                .with_optional_context(entities)
                .with_occurrences(ids)
                .build(),
        );
        total = total.saturating_add(ids.get());
    }

    total
}

#[cfg(test)]
mod tests {
    use crate::{stages::merged_records_report::report_merged_records, test_reporter::RecordingReporter};
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, resolver_code::ResolverCode};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use cassiopeia_reporter::backend::noop::NoopReporter;
    use cassiopeia_resolver::{field_path::FieldPath, merged_records::MergedRecords, record_merge::RecordMerge};
    use std::num::NonZeroU64;

    fn merge(entity_type: &str, id: &str, records: u64) -> RecordMerge {
        RecordMerge {
            id: format!("urn:ngsi-ld:{entity_type}:{id}").parse().unwrap(),
            entity_type: NameBuf::new(entity_type).unwrap(),
            records: NonZeroU64::new(records).unwrap(),
            field: FieldPath::new(&["temperature"]),
        }
    }

    #[test]
    fn an_empty_tally_says_nothing_and_counts_nothing() {
        assert_eq!(report_merged_records(&NoopReporter::new(), MergedRecords::new()), 0);
    }

    #[test]
    fn one_disagreeing_id_names_the_id_its_record_count_and_the_field() {
        let sink = RecordingReporter::new();
        let mut merged = MergedRecords::new();
        merged.record(merge("WeatherStation", "WeatherStation", 12));

        let total = report_merged_records(&sink, merged);

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Resolver(ResolverCode::RecordsMerged));
        assert_eq!(reported[0].code.to_string(), "resolver-records-merged");
        assert_eq!(
            reported[0].headline,
            "12 records resolved to `urn:ngsi-ld:WeatherStation:WeatherStation` and disagreed on `temperature`; one value of each disagreeing field was kept and the others were discarded"
        );
        assert_eq!(reported[0].occurrences, 1);
    }

    #[test]
    fn many_disagreeing_ids_of_one_type_are_one_diagnostic_naming_the_largest_and_counting_the_rest() {
        let sink = RecordingReporter::new();
        let mut merged = MergedRecords::new();
        for index in 0..10_000 {
            merged.record(merge("Station", &format!("s{index:05}"), 2));
        }
        merged.record(merge("Station", "busiest", 5));

        let total = report_merged_records(&sink, merged);

        let reported = sink.diagnostics();
        assert_eq!(total, 10_001);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 10_001);
        assert_eq!(
            reported[0].headline,
            "5 records resolved to `urn:ngsi-ld:Station:busiest` and disagreed on `temperature`; one value of each disagreeing field was kept and the others were discarded; the records of 10000 more `Station` entities disagreed too"
        );
    }

    #[test]
    fn one_further_disagreeing_id_is_named_in_the_singular() {
        let sink = RecordingReporter::new();
        let mut merged = MergedRecords::new();
        merged.record(merge("Station", "a", 2));
        merged.record(merge("Station", "b", 3));

        report_merged_records(&sink, merged);

        assert_eq!(
            sink.diagnostics()[0].headline,
            "3 records resolved to `urn:ngsi-ld:Station:b` and disagreed on `temperature`; one value of each disagreeing field was kept and the others were discarded; the records of 1 more `Station` entity disagreed too"
        );
    }

    #[test]
    fn each_entity_type_is_its_own_diagnostic() {
        let sink = RecordingReporter::new();
        let mut merged = MergedRecords::new();
        merged.record(merge("WeatherStation", "w", 2));
        merged.record(merge("BikeRentalShop", "b", 3));

        let total = report_merged_records(&sink, merged);

        let headlines: Vec<String> = sink.diagnostics().into_iter().map(|diagnostic| diagnostic.headline).collect();
        assert_eq!(total, 2);
        assert_eq!(
            headlines,
            [
                "3 records resolved to `urn:ngsi-ld:BikeRentalShop:b` and disagreed on `temperature`; one value of each disagreeing field was kept and the others were discarded",
                "2 records resolved to `urn:ngsi-ld:WeatherStation:w` and disagreed on `temperature`; one value of each disagreeing field was kept and the others were discarded",
            ]
        );
    }
}
