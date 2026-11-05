use cassiopeia_batch_sink::{occurrences::Occurrences, recurring::Recurring};
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode},
    context_field::ContextField,
    diagnostic::Diagnostic,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_extractor::dropped_null::DroppedNull;
use cassiopeia_ngsi_ld::entity::attribute_path::AttributePath;

/// The diagnostic naming one attribute whose values were written without the nulls their source
/// held at one site.
///
/// One diagnostic per attribute and site, however many records lost a null there. The headline names
/// the attribute because the concise form shows nothing else: the attribute also travels as a context
/// field, but that field is only rendered under `-v`.
pub(crate) fn dropped_null_diagnostic(dropped: DroppedNull, record: &Occurrences) -> Diagnostic {
    let headline = dropped.to_string();

    DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Extractor(ExtractorCode::NullDropped), headline)
        .with_context(ContextField::Attribute(AttributePath::top_level(dropped.attribute)))
        .with_occurrences(record.occurrences())
        .build()
}

#[cfg(test)]
mod tests {
    use crate::{
        stages::{batch_sink_report::report_sink, dropped_null_report::dropped_null_diagnostic},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode},
        context_field::ContextField,
    };
    use cassiopeia_extractor::dropped_nulls::DroppedNulls;
    use cassiopeia_ngsi_ld::{
        entity::{attribute_path::AttributePath, name::NameBuf},
        value::omitted_nulls::OmittedNulls,
    };
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn repeats_on_one_attribute_and_site_are_one_diagnostic_naming_the_attribute() {
        let sink = RecordingReporter::new();
        let dropped = DroppedNulls::new();
        let lost_an_item = OmittedNulls {
            array_items: 1,
            object_members: 0,
        };
        dropped.record(&name("tags"), lost_an_item);
        dropped.record(&name("tags"), lost_an_item);

        let total = report_sink(&sink, dropped, dropped_null_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 2);
        assert_eq!(reported[0].code, DiagnosticCode::Extractor(ExtractorCode::NullDropped));
        assert!(reported[0].headline.contains("`tags`"), "{}", reported[0].headline);
        assert_eq!(reported[0].defining, [ContextField::Attribute(AttributePath::top_level(name("tags")))]);
    }
}
