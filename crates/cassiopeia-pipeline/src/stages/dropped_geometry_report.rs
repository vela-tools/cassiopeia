use crate::stages::coded_error::CodedError;
use cassiopeia_batch_sink::{occurrences::Occurrences, recurring::Recurring};
use cassiopeia_diagnostic::{
    cause::causes_of,
    code::diagnostic_code::DiagnosticCode,
    context_field::ContextField,
    diagnostic::Diagnostic,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_extractor::dropped_geometry::DroppedGeometry;

/// The diagnostic naming one declaration's refused geometry conversions.
///
/// One diagnostic per declaration and refusal, however many records were refused the same way: the
/// refusal's own code says why, the headline names the declaration by its path from the top-level
/// attribute because the concise form shows nothing else, the path travels as context so two
/// declarations refused for the same reason stay two findings, and the count says how far it
/// spread.
pub(crate) fn dropped_geometry_diagnostic(dropped: DroppedGeometry, record: &Occurrences) -> Diagnostic {
    let code = DiagnosticCode::Geometry(dropped.refusal.code());
    let causes = causes_of(&dropped.refusal);

    DiagnosticBuilder::new(Severity::Warning, code, dropped.to_string())
        .with_causes(causes)
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
    use cassiopeia_geometry::{error::GeometryError, geometry::GeometryKind};
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn footprint_of(root: &str) -> AttributePath {
        AttributePath::new(name(root), vec![name("footprint")], Vec::new())
    }

    const POLYGON_TO_POINT: GeometryError = GeometryError::Uncoercible {
        origin: GeometryKind::Polygon,
        target: GeometryKind::Point,
    };

    #[test]
    fn repeats_of_one_refusal_on_one_attribute_are_one_diagnostic_under_the_refusal_s_code_naming_the_attribute() {
        let sink = RecordingReporter::new();
        let dropped = DroppedGeometries::new();
        dropped.record(AttributePath::top_level(name("location")), GeometryError::GeometryCollection);
        dropped.record(AttributePath::top_level(name("location")), GeometryError::GeometryCollection);

        let total = report_sink(&sink, dropped, dropped_geometry_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 2);
        assert_eq!(reported[0].code, DiagnosticCode::Geometry(GeometryCode::CollectionInadmissible));
        assert!(
            reported[0]
                .headline
                .starts_with("Attribute `location` dropped: a GeometryCollection is not an admissible GeoProperty value"),
            "{}",
            reported[0].headline
        );
        assert_eq!(reported[0].defining, [ContextField::Attribute(AttributePath::top_level(name("location")))]);
    }

    #[test]
    fn a_sub_attribute_is_named_by_its_path_in_the_headline_and_the_context() {
        let sink = RecordingReporter::new();
        let dropped = DroppedGeometries::new();
        dropped.record(footprint_of("temperature"), POLYGON_TO_POINT);

        report_sink(&sink, dropped, dropped_geometry_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(reported[0].code, DiagnosticCode::Geometry(GeometryCode::Uncoercible));
        assert_eq!(
            reported[0].headline,
            "Attribute `temperature.footprint` dropped: a Polygon cannot become a Point without a declared `geometry: { convert: ... }` conversion"
        );
        assert_eq!(reported[0].defining, [ContextField::Attribute(footprint_of("temperature"))]);
    }

    #[test]
    fn one_sub_attribute_name_refused_under_two_attributes_is_two_diagnostics() {
        let sink = RecordingReporter::new();
        let dropped = DroppedGeometries::new();
        dropped.record(footprint_of("temperature"), POLYGON_TO_POINT);
        dropped.record(footprint_of("humidity"), POLYGON_TO_POINT);

        report_sink(&sink, dropped, dropped_geometry_diagnostic);

        let mut headlines = sink
            .diagnostics()
            .into_iter()
            .map(|diagnostic| (diagnostic.occurrences, diagnostic.headline))
            .collect::<Vec<_>>();
        headlines.sort();
        assert_eq!(
            headlines,
            [
                (
                    1,
                    "Attribute `humidity.footprint` dropped: a Polygon cannot become a Point without a declared `geometry: { convert: ... }` conversion"
                        .to_owned()
                ),
                (
                    1,
                    "Attribute `temperature.footprint` dropped: a Polygon cannot become a Point without a declared `geometry: { convert: ... }` conversion"
                        .to_owned()
                ),
            ]
        );
    }
}
