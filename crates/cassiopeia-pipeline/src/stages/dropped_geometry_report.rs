use crate::stages::coded_error::CodedError;
use cassiopeia_batch_sink::{occurrences::Occurrences, recurring::Recurring};
use cassiopeia_diagnostic::{
    code::diagnostic_code::DiagnosticCode,
    context_field::ContextField,
    diagnostic::Diagnostic,
    diagnostic_builder::from_error,
    severity::Severity,
};
use cassiopeia_extractor::dropped_geometry::DroppedGeometry;

/// The diagnostic naming one attribute's refused geometry conversions.
///
/// One diagnostic per attribute and refusal, however many records were refused the same way: the
/// refusal's own code says why, the attribute travels as context, and the count says how far it
/// spread.
pub(crate) fn dropped_geometry_diagnostic(dropped: DroppedGeometry, record: &Occurrences) -> Diagnostic {
    from_error(Severity::Warning, DiagnosticCode::Geometry(dropped.refusal.code()), &dropped.refusal)
        .with_context(ContextField::Attribute(dropped.attribute))
        .with_occurrences(record.occurrences())
        .build()
}

#[cfg(test)]
mod tests {
    use crate::{
        stages::{batch_sink_report::report_sink, dropped_geometry_report::dropped_geometry_diagnostic},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, geometry_code::GeometryCode},
        context_field::ContextField,
    };
    use cassiopeia_extractor::dropped_geometries::DroppedGeometries;
    use cassiopeia_geometry::error::GeometryError;
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn repeats_of_one_refusal_on_one_attribute_are_one_diagnostic_under_the_refusal_s_code() {
        let sink = RecordingReporter::new();
        let dropped = DroppedGeometries::new();
        dropped.record(&name("location"), GeometryError::GeometryCollection);
        dropped.record(&name("location"), GeometryError::GeometryCollection);

        let total = report_sink(&sink, dropped, dropped_geometry_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 2);
        assert_eq!(reported[0].code, DiagnosticCode::Geometry(GeometryCode::CollectionInadmissible));
        assert_eq!(reported[0].defining, [ContextField::Attribute(name("location"))]);
    }
}
