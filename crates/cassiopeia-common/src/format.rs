use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use strum::Display;

/// The source formats Cassiopeia can ingest.
///
/// One enum is shared by the profiler, the collector, the ingestors, and the CLI so a format is
/// named the same way at every stage.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum, Display)]
#[serde(rename_all = "kebab-case")]
pub enum DataFormat {
    /// Detect the format from the file extension or the content.
    #[default]
    #[strum(serialize = "auto")]
    Auto,

    /// Comma-separated values.
    #[strum(serialize = "CSV")]
    Csv,

    /// A JSON array of objects.
    #[strum(serialize = "JSON")]
    Json,

    /// A `GeoJSON` `FeatureCollection`.
    #[serde(alias = "geojson")]
    #[value(name = "geojson")]
    #[strum(serialize = "GeoJSON")]
    GeoJson,

    /// Keyhole Markup Language.
    #[serde(alias = "kml")]
    #[value(name = "kml")]
    #[strum(serialize = "KML")]
    Kml,

    /// Zipped Keyhole Markup Language.
    #[serde(alias = "kmz")]
    #[value(name = "kmz")]
    #[strum(serialize = "KMZ")]
    Kmz,

    /// WMO GRIB gridded binary (meteorological/climate fields).
    ///
    /// One user-facing format with no edition number; the edition (GRIB1 vs GRIB2) is inspected at
    /// runtime by the profiler, and read from the header by the ingestor as a fallback.
    #[serde(alias = "grib")]
    #[value(name = "grib")]
    #[strum(serialize = "GRIB")]
    Grib,

    /// ESRI Shapefile: a geometry `.shp` with its companion `.dbf`/`.shx`/`.prj`/`.cpg` files.
    ///
    /// One user-facing format covering both a bare local `.shp` (its siblings resolved from the same
    /// directory) and a `.zip` bundle of the companion set; the carrier is chosen at runtime by the
    /// ingestor from the leading bytes, not the declared format.
    #[serde(alias = "shapefile")]
    #[value(name = "shapefile")]
    #[strum(serialize = "Shapefile")]
    Shapefile,

    /// Generic XML.
    ///
    /// Schemaless like JSON: the ingestor discovers records at parse time from a deterministic
    /// convention (a repeated child element under the root), so there is no configured parsing knob
    /// and no attached format metadata. KML, a specific XML dialect, is a distinct format detected
    /// ahead of this generic one.
    #[serde(alias = "xml")]
    #[value(name = "xml")]
    #[strum(serialize = "XML")]
    Xml,
}

impl DataFormat {
    /// The file extension the collector uses when it has to name a downloaded source.
    ///
    /// `Auto` has no extension of its own, because the format is not yet known when it is set.
    #[must_use]
    pub const fn extension(&self) -> &'static str {
        match self {
            DataFormat::Auto => "",
            DataFormat::Csv => "csv",
            DataFormat::Json => "json",
            DataFormat::GeoJson => "geojson",
            DataFormat::Kml => "kml",
            DataFormat::Kmz => "kmz",
            DataFormat::Grib => "grib",
            DataFormat::Shapefile => "shp",
            DataFormat::Xml => "xml",
        }
    }

    /// Whether this format can pack several logical record collections into one source.
    ///
    /// KML nests placemarks under sibling `<Folder>`s, and a shapefile `.zip` bundle can hold several
    /// `.shp` layers (each becoming one collection), so one source can carry collections a manifest
    /// routes to different entity types; the row-and-object formats are always a single collection.
    /// GRIB is single-collection too: its parameters are pivoted into columns of one location record
    /// (like a CSV row), not split into separate collections. This is the static, format-stated
    /// guarantee used for an early manifest check; when the format is `Auto`/omitted the runtime router
    /// in the expander is the backstop instead.
    #[must_use]
    pub const fn supports_collections(&self) -> bool {
        match self {
            DataFormat::Kml | DataFormat::Kmz | DataFormat::Shapefile => true,
            DataFormat::Auto | DataFormat::Csv | DataFormat::Json | DataFormat::GeoJson | DataFormat::Grib | DataFormat::Xml => false,
        }
    }

    /// The format as an `Option`, where `Auto` becomes `None`.
    ///
    /// `Auto` is a CLI-level placeholder for "not stated"; every stage past argument parsing works
    /// with `Option<DataFormat>` instead.
    #[must_use]
    pub const fn to_option(self) -> Option<DataFormat> {
        match self {
            DataFormat::Auto => None,
            DataFormat::Csv
            | DataFormat::Json
            | DataFormat::GeoJson
            | DataFormat::Kml
            | DataFormat::Kmz
            | DataFormat::Grib
            | DataFormat::Shapefile
            | DataFormat::Xml => Some(self),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::format::DataFormat;

    #[test]
    fn auto_carries_no_extension_and_no_concrete_format() {
        assert_eq!(DataFormat::Auto.extension(), "");
        assert_eq!(DataFormat::Auto.to_option(), None);
    }

    #[test]
    fn a_concrete_format_survives_the_conversion_to_option() {
        assert_eq!(DataFormat::Csv.to_option(), Some(DataFormat::Csv));
    }

    #[test]
    fn display_uses_the_conventional_spelling_of_each_format() {
        assert_eq!(DataFormat::GeoJson.to_string(), "GeoJSON");
    }

    #[test]
    fn only_the_kml_family_supports_multiple_collections() {
        assert!(DataFormat::Kml.supports_collections());
        assert!(DataFormat::Kmz.supports_collections());
        assert!(!DataFormat::Auto.supports_collections());
        assert!(!DataFormat::Csv.supports_collections());
        assert!(!DataFormat::Json.supports_collections());
        assert!(!DataFormat::GeoJson.supports_collections());
        // GRIB pivots parameters into columns of one location record, so it is single-collection.
        assert!(!DataFormat::Grib.supports_collections());
        // XML is single-collection like JSON; records come from one repeated child element.
        assert!(!DataFormat::Xml.supports_collections());
    }

    #[test]
    fn grib_carries_its_extension_and_survives_the_conversion_to_option() {
        assert_eq!(DataFormat::Grib.extension(), "grib");
        assert_eq!(DataFormat::Grib.to_option(), Some(DataFormat::Grib));
    }

    #[test]
    fn shapefile_carries_its_extension_supports_collections_and_survives_to_option() {
        assert_eq!(DataFormat::Shapefile.extension(), "shp");
        assert!(DataFormat::Shapefile.supports_collections());
        assert_eq!(DataFormat::Shapefile.to_option(), Some(DataFormat::Shapefile));
    }

    #[test]
    fn shapefile_deserializes_from_its_lowercase_alias_and_displays_conventionally() {
        assert_eq!(serde_json::from_str::<DataFormat>(r#""shapefile""#).unwrap(), DataFormat::Shapefile);
        assert_eq!(DataFormat::Shapefile.to_string(), "Shapefile");
    }

    #[test]
    fn xml_carries_its_extension_and_survives_the_conversion_to_option() {
        assert_eq!(DataFormat::Xml.extension(), "xml");
        assert_eq!(DataFormat::Xml.to_option(), Some(DataFormat::Xml));
    }

    #[test]
    fn xml_deserializes_from_its_lowercase_alias_and_displays_conventionally() {
        assert_eq!(serde_json::from_str::<DataFormat>(r#""xml""#).unwrap(), DataFormat::Xml);
        assert_eq!(DataFormat::Xml.to_string(), "XML");
    }

    #[test]
    fn geojson_deserializes_from_both_the_kebab_case_and_the_conventional_spelling() {
        assert_eq!(serde_json::from_str::<DataFormat>(r#""geo-json""#).unwrap(), DataFormat::GeoJson);
        assert_eq!(serde_json::from_str::<DataFormat>(r#""geojson""#).unwrap(), DataFormat::GeoJson);
    }
}
