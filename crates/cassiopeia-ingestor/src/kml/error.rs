/// Errors that can occur while parsing KML or KMZ input.
#[derive(Debug, thiserror::Error)]
pub enum KmlIngestError {
    /// The KML/KMZ ingestor was handed an in-memory byte payload; it needs a file.
    #[error("the KML ingestor requires a file, not bytes")]
    RequiresFile,

    /// The input could not be parsed as KML.
    #[error(transparent)]
    Read(#[from] ::kml::Error),
}
