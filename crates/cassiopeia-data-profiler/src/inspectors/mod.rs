use crate::{
    content::Content,
    error::Result,
    inspectors::{csv::CsvInspector, grib::GribInspector, sensorthings::SensorThingsInspector},
    metadata::{CsvMetadata, FormatMetadata},
};
use cassiopeia_common::format::DataFormat;
use cassiopeia_csv_dialect::statement::DialectStatement;

pub mod csv;
pub mod grib;
pub mod sensorthings;

/// Runs the structural inspector for `format` over `content`, extracting the format-specific metadata.
///
/// This is the shared entry point both the auto-detect path (through each format's `Detector`) and the
/// declared-format path (through the passthrough profiler) route to, so a format's inspector runs the
/// same way regardless of how the format was chosen: a declared CSV keeps its real dialect and a
/// declared GRIB keeps its edition, exactly as the detected paths attach them. A CSV yields its
/// dialect and a GRIB its edition; a SensorThings snapshot is checked for its header line and yields
/// nothing, since its ingestor reads the header it needs; a self-describing or schemaless format (JSON, `GeoJSON`, KML, KMZ,
/// Shapefile, `GeoPackage`, `NetCDF`, XML) has no structural inspector and yields `None`, its record shape
/// deferred to the ingestor.
///
/// `statement` is what the source's manifest input states about a CSV dialect: the inspector takes the
/// stated members and detects the rest. It describes a CSV alone, so every other format ignores it.
///
/// # Errors
///
/// Returns [`crate::error::DataProfilerError`] when the CSV dialect cannot be inspected, or when a
/// SensorThings snapshot has no header line Cassiopeia reads.
pub fn inspect(format: DataFormat, content: &Content<'_>, statement: &DialectStatement) -> Result<Option<FormatMetadata>> {
    match format {
        DataFormat::Csv => {
            let dialect = CsvInspector::new(statement).inspect(content)?;
            Ok(Some(FormatMetadata::Csv(CsvMetadata::new(dialect))))
        }
        DataFormat::Grib => Ok(GribInspector::inspect(content.bytes()).map(FormatMetadata::Grib)),
        DataFormat::SensorThings => {
            SensorThingsInspector::inspect(content.bytes())?;
            Ok(None)
        }
        DataFormat::Auto
        | DataFormat::Json
        | DataFormat::GeoJson
        | DataFormat::Kml
        | DataFormat::Kmz
        | DataFormat::Shapefile
        | DataFormat::GeoPackage
        | DataFormat::NetCdf
        | DataFormat::Xml => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use crate::{content::Content, inspectors::inspect};
    use cassiopeia_common::format::DataFormat;
    use cassiopeia_csv_dialect::{dialect_byte::DialectByte, statement::DialectStatement};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_declared_csv_is_inspected_for_its_dialect() {
        let metadata = inspect(DataFormat::Csv, &Content::new(b"id;name\n1;alpha\n2;beta\n"), &DialectStatement::NOTHING)
            .unwrap()
            .unwrap();
        assert_eq!(metadata.as_csv().unwrap().dialect().delimiter, DialectByte::SEMICOLON);
    }

    #[test]
    fn a_declared_grib_is_inspected_for_its_edition() {
        let metadata = inspect(
            DataFormat::Grib,
            &Content::new(b"GRIB\x00\x00\x00\x02\x00\x00\x00\x00"),
            &DialectStatement::NOTHING,
        )
        .unwrap()
        .unwrap();
        assert!(metadata.as_grib().is_some());
    }

    #[test]
    fn a_self_describing_format_has_no_inspector() {
        assert!(inspect(DataFormat::Json, &Content::new(b"{}"), &DialectStatement::NOTHING).unwrap().is_none());
        assert!(
            inspect(DataFormat::Shapefile, &Content::new(b"anything"), &DialectStatement::NOTHING)
                .unwrap()
                .is_none()
        );
        assert!(
            inspect(DataFormat::GeoPackage, &Content::new(b"anything"), &DialectStatement::NOTHING)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_declared_netcdf_has_no_inspector() {
        assert!(
            inspect(DataFormat::NetCdf, &Content::new(b"CDF\x01\x00\x00\x00\x00"), &DialectStatement::NOTHING)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_declared_sensorthings_file_without_header_is_refused() {
        let error = inspect(DataFormat::SensorThings, &Content::new(br#"{"value":[]}"#), &DialectStatement::NOTHING).unwrap_err();

        assert!(error.to_string().contains(r#"{"sensorthingsSnapshot":1,"#), "{error}");
    }

    #[test]
    fn a_declared_snapshot_with_its_header_is_accepted() {
        let snapshot =
            br#"{"sensorthingsSnapshot":1,"serviceRoot":"https://h.example/v1.1","version":"v1.1","read":"latest","collectedAt":"2026-11-12T10:00:00Z"}"#;

        assert!(
            inspect(DataFormat::SensorThings, &Content::new(snapshot), &DialectStatement::NOTHING)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn an_empty_declared_csv_surfaces_the_inspection_error() {
        assert!(inspect(DataFormat::Csv, &Content::new(b""), &DialectStatement::NOTHING).is_err());
    }
}
