use cassiopeia_batch_sink::{counted_example::CountedExample, recurring::Recurring};
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, geometry_code::GeometryCode},
    context_field::ContextField,
    diagnostic::Diagnostic,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_mapping::attribute_path::AttributePath;

/// The diagnostic naming one declaration whose geometries carried a position outside WGS 84.
///
/// One diagnostic per declaration, however many records lost their geometry. The headline names
/// the declaration by its path, the first position refused with where it sits in its geometry, and
/// the usual reasons coordinates leave the range, because the concise form shows nothing else; the
/// example rides on the headline, which stays outside the diagnostic's identity so it cannot split
/// one group into a line per batch.
pub(crate) fn out_of_range_geometry_diagnostic(attribute: AttributePath, record: &CountedExample) -> Diagnostic {
    let headline = format!(
        "Attribute `{attribute}` dropped: {}; GeoJSON coordinates are WGS 84 longitude and latitude in decimal degrees (RFC 7946 section 4), so the source may be in a coordinate reference system it does not declare, have longitude and latitude swapped, or need another conversion or scale",
        record.example
    );

    DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Geometry(GeometryCode::CoordinatesOutOfRange), headline)
        .with_context(ContextField::Attribute(attribute.into_root()))
        .with_occurrences(record.occurrences())
        .build()
}

#[cfg(test)]
mod tests {
    use crate::{
        stages::{batch_sink_report::report_sink, out_of_range_geometry_report::out_of_range_geometry_diagnostic},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, geometry_code::GeometryCode},
        context_field::ContextField,
    };
    use cassiopeia_extractor::out_of_range_geometries::OutOfRangeGeometries;
    use cassiopeia_geometry::{
        geometry::NgsiLdGeometry,
        wgs84::{OutOfRangePosition, check_range},
    };
    use cassiopeia_mapping::attribute_path::AttributePath;
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn outside(coordinates: [f64; 2]) -> OutOfRangePosition {
        let point = NgsiLdGeometry::Point {
            coordinates: coordinates.into(),
        };

        check_range(&point).expect_err("the point is outside WGS 84")
    }

    #[test]
    fn repeats_on_one_declaration_are_one_diagnostic_naming_the_first_position_and_the_usual_causes() {
        let sink = RecordingReporter::new();
        let geometries = OutOfRangeGeometries::new();
        geometries.record(AttributePath::new(name("location"), Vec::new(), Vec::new()), &outside([2600.5, 341_870.2]));
        geometries.record(AttributePath::new(name("location"), Vec::new(), Vec::new()), &outside([16.37, 91.0]));

        let total = report_sink(&sink, geometries, out_of_range_geometry_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 2);
        assert_eq!(reported[0].code, DiagnosticCode::Geometry(GeometryCode::CoordinatesOutOfRange));
        assert_eq!(
            reported[0].headline,
            "Attribute `location` dropped: the position `[2600.5, 341870.2]` at `coordinates` has a longitude outside WGS 84; GeoJSON coordinates are WGS 84 longitude and latitude in decimal degrees (RFC 7946 section 4), so the source may be in a coordinate reference system it does not declare, have longitude and latitude swapped, or need another conversion or scale"
        );
        assert_eq!(reported[0].defining, [ContextField::Attribute(name("location"))]);
    }

    #[test]
    fn a_sub_attribute_is_named_by_its_path_and_its_top_level_attribute_is_the_context() {
        let sink = RecordingReporter::new();
        let geometries = OutOfRangeGeometries::new();
        geometries.record(
            AttributePath::new(name("observation"), vec![name("location")], Vec::new()),
            &outside([0.0, -95.0]),
        );

        report_sink(&sink, geometries, out_of_range_geometry_diagnostic);

        let reported = sink.diagnostics();
        assert!(
            reported[0]
                .headline
                .starts_with("Attribute `observation.location` dropped: the position `[0, -95]` at `coordinates` has a latitude outside WGS 84;"),
            "{}",
            reported[0].headline
        );
        assert_eq!(reported[0].defining, [ContextField::Attribute(name("observation"))]);
    }
}
