use cassiopeia_batch_sink::{counted_example::CountedExample, recurring::Recurring};
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode},
    context_field::ContextField,
    diagnostic::Diagnostic,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};

/// The diagnostic naming one attribute whose declarations were dropped because their `datasetId`
/// was neither an absolute URI nor `@none` (ETSI GS CIM 009 v1.9.1 clause 4.5.5.1).
///
/// One diagnostic per attribute, however many records or instances lost a declaration to it and
/// however many distinct texts they carried, since a templated `datasetId` would otherwise name every
/// record. The headline names the attribute and the first text, because the concise form shows
/// nothing else; the text rides on the headline, which stays outside the diagnostic's identity so it
/// cannot split one group into a line per batch.
pub(crate) fn unusable_dataset_id_diagnostic(attribute: NameBuf, record: &CountedExample) -> Diagnostic {
    let headline = format!(
        "Attribute `{attribute}` dropped where its `datasetId` is `{}`, which is not an absolute URI",
        record.example
    );

    DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Extractor(ExtractorCode::DatasetIdNotAUri), headline)
        .with_context(ContextField::Attribute(AttributePath::top_level(attribute)))
        .with_occurrences(record.occurrences())
        .build()
}

#[cfg(test)]
mod tests {
    use crate::{
        stages::{batch_sink_report::report_sink, unusable_dataset_id_report::unusable_dataset_id_diagnostic},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode},
        context_field::ContextField,
    };
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
    use cassiopeia_transformer::unusable_dataset_ids::UnusableDatasetIds;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn repeats_of_one_attribute_are_one_warning_naming_the_attribute_and_the_first_text() {
        let sink = RecordingReporter::new();
        let unusable = UnusableDatasetIds::new();
        unusable.record(&name("temperature"), "gfs model");
        unusable.record(&name("temperature"), "gfs model");

        let total = report_sink(&sink, unusable, unusable_dataset_id_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Extractor(ExtractorCode::DatasetIdNotAUri));
        assert_eq!(reported[0].occurrences, 2);
        assert_eq!(
            reported[0].headline,
            "Attribute `temperature` dropped where its `datasetId` is `gfs model`, which is not an absolute URI"
        );
        assert_eq!(reported[0].defining, [ContextField::Attribute(AttributePath::top_level(name("temperature")))]);
    }
}
