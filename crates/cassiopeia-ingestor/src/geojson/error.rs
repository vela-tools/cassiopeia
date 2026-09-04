/// Errors that can occur while parsing `GeoJSON` input.
#[derive(Debug, thiserror::Error)]
pub enum GeoJsonIngestError {
    /// The `GeoJSON` ingestor was handed an in-memory byte payload; it needs a file.
    #[error("the GeoJSON ingestor requires a file, not bytes")]
    RequiresFile,

    /// A bare geometry (or any non-feature document) was supplied.
    #[error("unsupported GeoJSON input: expected a Feature or FeatureCollection")]
    UnsupportedInput,

    /// The input could not be parsed as `GeoJSON`.
    #[error(transparent)]
    Parse(#[from] ::geojson::Error),

    /// A feature id could not be re-encoded as JSON.
    #[error("failed to encode a feature id")]
    Encode(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use crate::geojson::error::GeoJsonIngestError;
    use std::{collections::BTreeMap, error::Error};

    #[test]
    fn an_encode_failure_exposes_the_serializer_error_as_its_cause() {
        // A JSON object member name has to be a string, so a tuple-keyed map cannot be encoded.
        let Err(failure) = serde_json::to_value(BTreeMap::from([((1_u8, 2_u8), 3_u8)])) else {
            panic!("a tuple-keyed map has no JSON form");
        };
        let error = GeoJsonIngestError::Encode(failure);

        assert!(error.source().is_some());
    }
}
