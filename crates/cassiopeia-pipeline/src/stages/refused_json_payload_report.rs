use cassiopeia_batch_sink::{occurrences::Occurrences, recurring::Recurring};
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode},
    context_field::ContextField,
    diagnostic::Diagnostic,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_extractor::refused_json_payload::RefusedJsonPayload;
use cassiopeia_ngsi_ld::entity::attribute_path::AttributePath;

/// The diagnostic naming one `JsonProperty` whose source was no JSON object or array of objects.
///
/// One diagnostic per attribute and refusal, however many records were refused the same way. The
/// headline names the attribute and the shape found, because the concise form shows nothing else.
pub(crate) fn refused_json_payload_diagnostic(refused: RefusedJsonPayload, record: &Occurrences) -> Diagnostic {
    let headline = refused.to_string();

    DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Extractor(ExtractorCode::JsonPayloadMisshapen), headline)
        .with_context(ContextField::Attribute(AttributePath::top_level(refused.attribute)))
        .with_occurrences(record.occurrences())
        .build()
}

#[cfg(test)]
mod tests {
    use crate::{
        stages::{batch_sink_report::report_sink, refused_json_payload_report::refused_json_payload_diagnostic},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode},
        context_field::ContextField,
    };
    use cassiopeia_extractor::refused_json_payloads::RefusedJsonPayloads;
    use cassiopeia_ngsi_ld::{
        entity::{attribute_path::AttributePath, name::NameBuf},
        value::json_payload_error::{JsonPayloadError, JsonShape},
    };
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn repeats_of_one_refusal_on_one_attribute_are_one_diagnostic_naming_the_attribute_and_the_shape() {
        let sink = RecordingReporter::new();
        let refused = RefusedJsonPayloads::new();
        refused.record(&name("payload"), JsonPayloadError::NotAnObject { found: JsonShape::String });
        refused.record(&name("payload"), JsonPayloadError::NotAnObject { found: JsonShape::String });

        let total = report_sink(&sink, refused, refused_json_payload_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Extractor(ExtractorCode::JsonPayloadMisshapen));
        assert!(reported[0].headline.contains("`payload`"), "{}", reported[0].headline);
        assert!(reported[0].headline.contains("a string"), "{}", reported[0].headline);
        assert_eq!(reported[0].defining, [ContextField::Attribute(AttributePath::top_level(name("payload")))]);
    }
}
