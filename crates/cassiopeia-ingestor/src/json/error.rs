use std::{io, path::PathBuf};

/// Errors that can occur while parsing JSON-array input.
#[derive(Debug, thiserror::Error)]
pub enum JsonIngestError {
    /// The JSON ingestor was handed an in-memory byte payload; it needs a file.
    #[error("the JSON ingestor requires a file, not bytes")]
    RequiresFile,

    /// Reading the file into memory failed.
    #[error("failed to read the JSON input at '{}'", path.display())]
    Read {
        /// The underlying operating-system error.
        #[source]
        source: io::Error,
        /// The file that could not be read.
        path: PathBuf,
    },

    /// The input was not valid JSON.
    #[error(transparent)]
    Parse(#[from] simd_json::Error),

    /// The input parsed as `GeoJSON`; the `GeoJSON` ingestor should be used instead.
    #[error("input is GeoJSON; use the GeoJSON ingestor instead")]
    WrongIngestorForGeoJson,

    /// An array or envelope element was not an object.
    #[error("expected every array element to be a JSON object")]
    ExpectedObject,

    /// The top-level value was a scalar or null; it can be neither a record nor a set of records.
    #[error("expected a top-level JSON object or an array of objects")]
    ExpectedObjectOrArray,
}

#[cfg(test)]
mod tests {
    use crate::json::error::JsonIngestError;
    use std::{error::Error, io, path::PathBuf};

    #[test]
    fn a_read_failure_names_the_file_and_keeps_the_operating_system_reason() {
        let error = JsonIngestError::Read {
            source: io::Error::other("input/output error"),
            path: PathBuf::from("/data/stations.json"),
        };

        assert!(error.to_string().contains("/data/stations.json"));
        assert_eq!(error.source().expect("a chained cause").to_string(), "input/output error");
    }
}
