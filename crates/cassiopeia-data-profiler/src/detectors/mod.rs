use crate::{
    detectors::{csv::CsvDetector, grib::GribDetector, json::JsonDetector, kml::KmlDetector, shapefile::ShapefileDetector, xml::XmlDetector},
    profile::Profile,
};

pub mod csv;
pub mod grib;
pub mod json;
pub mod kml;
pub mod shapefile;
pub mod xml;

/// A single-format detector: inspects a byte buffer and reports a [`Profile`] when it recognises it.
pub trait Detector: Send + Sync {
    /// Attempts to detect the format of `bytes`, returning a [`Profile`] on a match.
    ///
    /// # Arguments
    /// * `bytes` - The full content of the file or buffer being profiled.
    fn detect(&self, bytes: &[u8]) -> Option<Profile>;
}

/// The detectors run by [`crate::profile_path`] and [`crate::profile_bytes`], in order.
///
/// Order is significant: the cheapest and most specific check runs first. GRIB is an unambiguous
/// four-octet binary magic, so it leads; then the shapefile (its own four-octet file code, or a `.shp`
/// zip member), then KML and the JSON family. KML is itself XML, so the specific KML check must precede
/// the permissive generic-XML detector; that generic-XML detector in turn precedes the CSV detector,
/// which would otherwise accept many delimited text files. The slice is `'static`: no per-call
/// allocation occurs.
#[must_use]
pub const fn default_detectors() -> &'static [&'static dyn Detector] {
    &[&GribDetector, &ShapefileDetector, &KmlDetector, &JsonDetector, &XmlDetector, &CsvDetector]
}

#[cfg(test)]
mod tests {
    use crate::detectors::default_detectors;
    use cassiopeia_common::format::DataFormat;

    #[test]
    fn detectors_run_kml_then_json_then_xml_then_csv() {
        let formats: Vec<DataFormat> = [
            b"<?xml version=\"1.0\"?><kml xmlns=\"http://www.opengis.net/kml/2.2\"/>".as_slice(),
            br#"{"a":1}"#.as_slice(),
            b"<root/>".as_slice(),
            b"id,name\n1,alpha\n".as_slice(),
        ]
        .iter()
        .filter_map(|bytes| default_detectors().iter().find_map(|d| d.detect(bytes)))
        .map(|profile| *profile.format())
        .collect();

        // The KML sample is XML too, but the specific KML detector precedes the generic XML one.
        assert_eq!(formats, vec![DataFormat::Kml, DataFormat::Json, DataFormat::Xml, DataFormat::Csv]);
    }

    #[test]
    fn there_are_exactly_six_detectors() {
        assert_eq!(default_detectors().len(), 6);
    }

    #[test]
    fn grib_magic_is_detected_ahead_of_the_text_detectors() {
        let profile = default_detectors()
            .iter()
            .find_map(|d| d.detect(b"GRIB\x00\x00\x00\x02"))
            .expect("GRIB magic is recognised");
        assert_eq!(*profile.format(), DataFormat::Grib);
    }
}
