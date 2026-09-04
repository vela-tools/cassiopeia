use crate::{
    csv::error::CsvIngestError,
    geojson::error::GeoJsonIngestError,
    grib::error::GribIngestError,
    json::error::JsonIngestError,
    kml::error::KmlIngestError,
    shapefile::error::ShapefileIngestError,
    xml::error::XmlIngestError,
};
use std::{io, path::PathBuf};

/// Errors that can occur during data ingestion (parsing).
///
/// Transport-level failures (channel, file I/O, re-streaming) are modelled here
/// directly; each format's parsing failures live in that format's own error type
/// and are wrapped through the `#[from]` variants.
#[derive(Debug, thiserror::Error)]
pub enum IngestorError {
    /// The ingestor has already been consumed and cannot be streamed again.
    #[error("Cannot be streamed twice")]
    CannotBeStreamedTwice,

    /// The downstream channel was closed before a batch could be sent, which means the pipeline is
    /// shutting down.
    #[error("downstream channel closed")]
    ChannelClosed,

    /// An I/O error occurred while opening a file for parsing.
    #[error("I/O error at '{}'", path.display())]
    Io { source: io::Error, path: PathBuf },

    /// A CSV parsing error occurred.
    #[error(transparent)]
    Csv(#[from] CsvIngestError),

    /// A `GeoJSON` parsing error occurred.
    #[error(transparent)]
    GeoJson(#[from] GeoJsonIngestError),

    /// A GRIB parsing error occurred.
    #[error(transparent)]
    Grib(#[from] GribIngestError),

    /// A JSON parsing error occurred.
    #[error(transparent)]
    Json(#[from] JsonIngestError),

    /// A KML parsing error occurred.
    #[error(transparent)]
    Kml(#[from] KmlIngestError),

    /// A shapefile ingestion error occurred.
    #[error(transparent)]
    Shapefile(#[from] ShapefileIngestError),

    /// An XML parsing error occurred.
    #[error(transparent)]
    Xml(#[from] XmlIngestError),
}
