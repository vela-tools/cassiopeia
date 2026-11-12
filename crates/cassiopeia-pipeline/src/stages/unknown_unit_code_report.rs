use cassiopeia_batch_sink::{occurrences::Occurrences, recurring::Recurring};
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, transform_code::TransformCode},
    context_field::ContextField,
    diagnostic::Diagnostic,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
    unit_text::UnitText,
};
use cassiopeia_ngsi_ld::entity::attribute_path::AttributePath;
use cassiopeia_transformer::unknown_unit_code::UnknownUnitCode;

/// What to change, carried as the diagnostic's cause so the concise form shows it under the headline.
///
/// Sources rarely hold a UN/CEFACT code, and the `cefact_unit` filter is what turns the notations they
/// do hold into one.
const HINT: &str = "translate a unit symbol, name, UCUM code or QUDT unit IRI into its code with `| cefact_unit`";

/// The diagnostic naming one attribute that lost its `unitCode` to one text.
///
/// One diagnostic per attribute and text, however many records carried them: the text is part of the
/// failure, carried as a defining context field, because two texts are two things to fix, while the
/// records that repeat one are counted. The headline names both, since the concise form shows no
/// context field and a reader who cannot see the text has nothing to go and fix.
pub(crate) fn unknown_unit_code_diagnostic(UnknownUnitCode { attribute, text }: UnknownUnitCode, record: &Occurrences) -> Diagnostic {
    let headline = format!("Attribute `{attribute}` is emitted without `unitCode`: `{text}` is not a UN/CEFACT Recommendation 20 code");

    DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Transform(TransformCode::UnitCodeUnknown), headline)
        .with_cause(HINT)
        .with_context(ContextField::Attribute(AttributePath::top_level(attribute)))
        .with_context(ContextField::UnitText(UnitText::new(text)))
        .with_occurrences(record.occurrences())
        .build()
}

#[cfg(test)]
mod tests {
    use crate::{
        stages::{batch_sink_report::report_sink, unknown_unit_code_report::unknown_unit_code_diagnostic},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_batch_sink::{occurrences::Occurrences, recurring::Recurring};
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, transform_code::TransformCode},
        context_field::ContextField,
        diagnostic::Diagnostic,
        reason::Sighting,
        reason_tally::ReasonTally,
        severity::Severity,
        unit_text::UnitText,
    };
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
    use cassiopeia_transformer::{unknown_unit_code::UnknownUnitCode, unknown_unit_codes::UnknownUnitCodes};
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    /// The diagnostic for `attribute` losing its `unitCode` to `text` on `occurrences` records.
    fn diagnostic(attribute: &str, text: &str, occurrences: u64) -> Diagnostic {
        let mut record = Occurrences::first();
        for _ in 1..occurrences {
            record.count_another();
        }
        let key = UnknownUnitCode {
            attribute: name(attribute),
            text: Box::from(text),
        };

        unknown_unit_code_diagnostic(key, &record)
    }

    #[test]
    fn unknown_unit_code_diagnostic_names_attribute_and_text() {
        let sink = RecordingReporter::new();
        let unknown = UnknownUnitCodes::new();
        unknown.record(&name("pm10"), "ug/m3x");
        unknown.record(&name("pm10"), "ug/m3x");

        let total = report_sink(&sink, unknown, unknown_unit_code_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Transform(TransformCode::UnitCodeUnknown));
        assert_eq!(
            reported[0].headline,
            "Attribute `pm10` is emitted without `unitCode`: `ug/m3x` is not a UN/CEFACT Recommendation 20 code"
        );
        assert_eq!(reported[0].occurrences, 2);
        assert_eq!(
            reported[0].defining,
            [
                ContextField::Attribute(AttributePath::top_level(name("pm10"))),
                ContextField::UnitText(UnitText::new("ug/m3x")),
            ]
        );
        assert!(reported[0].causes[0].contains("| cefact_unit"), "{:?}", reported[0].causes);
    }

    #[test]
    fn an_unknown_unit_code_is_a_warning() {
        assert_eq!(diagnostic("pm10", "ug/m3x", 1).severity(), Severity::Warning);
    }

    #[test]
    fn two_unit_notations_are_two_identities() {
        let tally = ReasonTally::new();

        assert_eq!(tally.record(&diagnostic("pm10", "ug/m3", 1)), Sighting::First);
        assert_eq!(tally.record(&diagnostic("pm10", "GQQ", 1)), Sighting::First);
        assert_eq!(tally.record(&diagnostic("pm10", "ug/m3", 3)), Sighting::Repeat);
    }
}
