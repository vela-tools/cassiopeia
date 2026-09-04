#[cfg(any(feature = "grib1", feature = "grib2-full"))]
use eccodes::errors::CodesError;
use grib::GribError;
#[cfg(any(feature = "grib1", feature = "grib2-full"))]
use std::num::TryFromIntError;

/// Errors that can occur while parsing GRIB input.
///
/// File-open and header-read I/O is reported by the transport-level [`crate::error::IngestorError`]
/// (which carries the path); this type covers only GRIB-specific failures.
#[derive(Debug, thiserror::Error)]
pub enum GribIngestError {
    /// The GRIB ingestor was handed an in-memory byte payload; it needs a file.
    ///
    /// GRIB is decoded from a seekable reader (grib-rs seeks between sections; ecCodes opens a path),
    /// so the ingestor reads from a file on disk rather than a byte buffer.
    #[error("the GRIB ingestor requires a file, not bytes")]
    RequiresFile,

    /// The file declares a GRIB edition this build does not decode.
    ///
    /// GRIB2 is always decodable (grib-rs, or ecCodes with `grib2-full`); GRIB1 is decoded when the
    /// `grib1` feature is enabled (ecCodes). This fires for GRIB1 in a build compiled without that
    /// feature, and for any edition other than 1 or 2.
    #[error("unsupported GRIB edition: {0}")]
    UnsupportedEdition(u8),

    /// The input could not be parsed as GRIB2 by grib-rs.
    #[error(transparent)]
    Parse(#[from] GribError),

    /// ecCodes failed to open or decode a GRIB message.
    #[cfg(any(feature = "grib1", feature = "grib2-full"))]
    #[error(transparent)]
    Eccodes(#[from] CodesError),

    /// A GRIB2 parameter identity code read from ecCodes did not fit the byte range the WMO tables
    /// define for it, so the message is malformed.
    #[cfg(any(feature = "grib1", feature = "grib2-full"))]
    #[error("GRIB parameter identity code out of range: {0}")]
    ParameterCode(#[from] TryFromIntError),
}
