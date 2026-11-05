use cassiopeia_batch_sink::{counted_example::CountedExample, recurring::Recurring};
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode},
    context_field::ContextField,
    diagnostic::Diagnostic,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_extractor::{required_value::RequiredValue, unusable_value::UnusableValue};

/// The code a value its kind cannot hold, or its transformation cannot read, is published under.
///
/// A value that spells no boolean, one that spells no number, a non-IRI vocabulary value, a value
/// that is no language map, text that is not JSON, a value that is no JSON object, and a value that
/// spells no geometry are different mistakes with different fixes, so each publishes its own code.
/// An integer and a float share one, since both read a number and differ only in its kind.
const fn code(required: RequiredValue) -> DiagnosticCode {
    match required {
        RequiredValue::Boolean => DiagnosticCode::Extractor(ExtractorCode::BooleanUnreadable),
        RequiredValue::Integer | RequiredValue::Float => DiagnosticCode::Extractor(ExtractorCode::NumberUnreadable),
        RequiredValue::Iri => DiagnosticCode::Extractor(ExtractorCode::VocabNotAnIri),
        RequiredValue::LanguageMap => DiagnosticCode::Extractor(ExtractorCode::LanguageMapMisshapen),
        RequiredValue::Json => DiagnosticCode::Extractor(ExtractorCode::JsonTextUnreadable),
        RequiredValue::JsonObject => DiagnosticCode::Extractor(ExtractorCode::ObjectMisshapen),
        RequiredValue::Geometry => DiagnosticCode::Extractor(ExtractorCode::GeometryUnreadable),
    }
}

/// The diagnostic naming one declaration whose values its kind could not hold or its transformation
/// could not read.
///
/// One diagnostic per declaration and requirement, however many records lost their value and
/// however many distinct values they carried. The headline names the declaration by its path, the
/// first value, and what the value had to be, because the concise form shows nothing else; the
/// example rides on the headline, which stays outside the diagnostic's identity so it cannot split
/// one group into a line per batch.
pub(crate) fn unusable_value_diagnostic(UnusableValue { attribute, required }: UnusableValue, record: &CountedExample) -> Diagnostic {
    let headline = format!("Attribute `{attribute}` dropped: `{}` is not {required}", record.example);

    DiagnosticBuilder::new(Severity::Warning, code(required), headline)
        .with_context(ContextField::Attribute(attribute.root().clone()))
        .with_occurrences(record.occurrences())
        .build()
}

#[cfg(test)]
mod tests {
    use crate::{
        stages::{batch_sink_report::report_sink, unusable_value_report::unusable_value_diagnostic},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode},
        context_field::ContextField,
    };
    use cassiopeia_extractor::{attribute_path::AttributePath, required_value::RequiredValue, unusable_values::UnusableValues};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn repeats_of_one_vocab_refusal_are_one_diagnostic_naming_the_attribute_the_value_and_the_requirement() {
        let sink = RecordingReporter::new();
        let unusable = UnusableValues::new();
        unusable.record(AttributePath::new(name("category"), Vec::new(), Vec::new()), RequiredValue::Iri, "not an iri");
        unusable.record(AttributePath::new(name("category"), Vec::new(), Vec::new()), RequiredValue::Iri, "also not one");

        let total = report_sink(&sink, unusable, unusable_value_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 2);
        assert_eq!(reported[0].code, DiagnosticCode::Extractor(ExtractorCode::VocabNotAnIri));
        assert_eq!(reported[0].headline, "Attribute `category` dropped: `not an iri` is not an IRI");
        assert_eq!(reported[0].defining, [ContextField::Attribute(name("category"))]);
    }

    #[test]
    fn a_sub_attribute_is_named_by_its_path_and_its_top_level_attribute_is_the_context() {
        let sink = RecordingReporter::new();
        let unusable = UnusableValues::new();
        unusable.record(
            AttributePath::new(name("temperature"), vec![name("label")], Vec::new()),
            RequiredValue::LanguageMap,
            "plain text",
        );

        report_sink(&sink, unusable, unusable_value_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(reported[0].code, DiagnosticCode::Extractor(ExtractorCode::LanguageMapMisshapen));
        assert_eq!(
            reported[0].headline,
            "Attribute `temperature.label` dropped: `plain text` is not a language map of text keyed by language tag"
        );
        assert_eq!(reported[0].defining, [ContextField::Attribute(name("temperature"))]);
    }

    #[test]
    fn text_that_is_no_json_and_a_value_that_is_no_json_object_are_each_named_under_their_own_code() {
        let sink = RecordingReporter::new();
        let unusable = UnusableValues::new();
        unusable.record(AttributePath::new(name("payload"), Vec::new(), Vec::new()), RequiredValue::Json, "{not json");
        unusable.record(AttributePath::new(name("settings"), Vec::new(), Vec::new()), RequiredValue::JsonObject, "[1,2]");

        report_sink(&sink, unusable, unusable_value_diagnostic);

        let mut named = sink
            .diagnostics()
            .into_iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.headline))
            .collect::<Vec<_>>();
        named.sort_by(|left, right| left.1.cmp(&right.1));
        assert_eq!(
            named,
            [
                (
                    DiagnosticCode::Extractor(ExtractorCode::JsonTextUnreadable),
                    "Attribute `payload` dropped: `{not json` is not JSON".to_owned()
                ),
                (
                    DiagnosticCode::Extractor(ExtractorCode::ObjectMisshapen),
                    "Attribute `settings` dropped: `[1,2]` is not a JSON object".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn a_value_that_is_no_geometry_is_named_under_its_own_code() {
        let sink = RecordingReporter::new();
        let unusable = UnusableValues::new();
        unusable.record(
            AttributePath::new(name("location"), Vec::new(), Vec::new()),
            RequiredValue::Geometry,
            r#"{"street":"Ring"}"#,
        );

        report_sink(&sink, unusable, unusable_value_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Extractor(ExtractorCode::GeometryUnreadable));
        assert_eq!(
            reported[0].headline,
            r#"Attribute `location` dropped: `{"street":"Ring"}` is not a GeoJSON geometry"#
        );
    }

    #[test]
    fn each_scalar_requirement_is_reported_under_its_code_naming_the_value_and_the_requirement() {
        for (required, value, code, requirement) in [
            (RequiredValue::Boolean, "maybe", ExtractorCode::BooleanUnreadable, "a boolean"),
            (RequiredValue::Integer, "abc", ExtractorCode::NumberUnreadable, "an integer"),
            (RequiredValue::Float, "x", ExtractorCode::NumberUnreadable, "a number"),
        ] {
            let unusable = UnusableValues::new();
            let sink = RecordingReporter::new();
            unusable.record(AttributePath::new(name("count"), Vec::new(), Vec::new()), required, value);

            report_sink(&sink, unusable, unusable_value_diagnostic);

            let reported = sink.diagnostics();
            assert_eq!(reported.len(), 1, "{required}");
            assert_eq!(reported[0].code, DiagnosticCode::Extractor(code), "{required}");
            assert_eq!(reported[0].headline, format!("Attribute `count` dropped: `{value}` is not {requirement}"));
        }
    }
}
