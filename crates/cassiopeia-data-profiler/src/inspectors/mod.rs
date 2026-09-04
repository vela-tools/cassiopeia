use crate::{
    error::Result,
    inspectors::{csv::CsvInspector, grib::GribInspector},
    metadata::{CsvMetadata, FormatMetadata},
};
use cassiopeia_common::format::DataFormat;

pub mod csv;
pub mod grib;

/// Runs the structural inspector for `format` over `bytes`, extracting the format-specific metadata.
///
/// This is the shared entry point both the auto-detect path (through each format's `Detector`) and the
/// declared-format path (through the passthrough profiler) route to, so a format's inspector runs the
/// same way regardless of how the format was chosen: a declared CSV keeps its real dialect and a
/// declared GRIB keeps its edition, exactly as the detected paths attach them. A CSV yields its
/// dialect and a GRIB its edition; a self-describing or schemaless format (JSON, `GeoJSON`, KML, KMZ,
/// Shapefile, XML) has no structural inspector and yields `None`, its record shape deferred to the
/// ingestor.
///
/// # Errors
///
/// Returns [`crate::error::DataProfilerError`] when the CSV dialect cannot be inspected.
pub fn inspect(format: DataFormat, bytes: &[u8]) -> Result<Option<FormatMetadata>> {
    match format {
        DataFormat::Csv => {
            let dialect = CsvInspector::new().inspect(bytes)?;
            Ok(Some(FormatMetadata::Csv(CsvMetadata::new(dialect))))
        }
        DataFormat::Grib => Ok(GribInspector::inspect(bytes).map(FormatMetadata::Grib)),
        DataFormat::Auto | DataFormat::Json | DataFormat::GeoJson | DataFormat::Kml | DataFormat::Kmz | DataFormat::Shapefile | DataFormat::Xml => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use crate::inspectors::inspect;
    use cassiopeia_common::format::DataFormat;

    #[test]
    fn a_declared_csv_is_inspected_for_its_dialect() {
        let metadata = inspect(DataFormat::Csv, b"id;name\n1;alpha\n2;beta\n").unwrap().unwrap();
        assert_eq!(metadata.as_csv().unwrap().dialect().delimiter, b';');
    }

    #[test]
    fn a_declared_grib_is_inspected_for_its_edition() {
        let metadata = inspect(DataFormat::Grib, b"GRIB\x00\x00\x00\x02\x00\x00\x00\x00").unwrap().unwrap();
        assert!(metadata.as_grib().is_some());
    }

    #[test]
    fn a_self_describing_format_has_no_inspector() {
        assert!(inspect(DataFormat::Json, b"{}").unwrap().is_none());
        assert!(inspect(DataFormat::Shapefile, b"anything").unwrap().is_none());
    }

    #[test]
    fn an_empty_declared_csv_surfaces_the_inspection_error() {
        assert!(inspect(DataFormat::Csv, b"").is_err());
    }
}
