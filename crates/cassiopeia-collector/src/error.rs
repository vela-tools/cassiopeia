use cassiopeia_common::captured_body::CapturedBody;
use http::StatusCode;
use std::{io, path::PathBuf};
use url::Url;

/// The cap on a captured error-response body, which exists to be read by a person.
pub const ERROR_BODY_CAP: usize = 2 * 1024;

/// Errors that can occur during data collection.
#[derive(Debug, thiserror::Error)]
pub enum CollectorError {
    /// An HTTP request never completed.
    #[error("HTTP request to '{url}' failed")]
    Http {
        /// The transport failure reported by the HTTP layer.
        #[source]
        source: reqwest::Error,
        /// The URL the request targeted.
        url: Url,
    },

    /// The server answered with a non-success status.
    ///
    /// The body is kept rather than discarded: a 4xx or 5xx answers with the server's own
    /// explanation, and throwing it away leaves a failed download with nothing to act on.
    #[error("'{url}' answered {status}")]
    HttpStatus {
        /// The URL the request targeted.
        url: Url,
        /// The status the server answered with.
        status: StatusCode,
        /// The server's explanation, as far as it was captured. Boxed so a captured body does not
        /// enlarge every `Result` in the collector and every pipeline error that wraps one.
        body: Box<CapturedBody>,
    },

    /// An I/O error occurred while accessing a file.
    #[error("I/O error at '{}'", path.display())]
    Io {
        /// The underlying operating-system error.
        #[source]
        source: io::Error,
        /// The path the operation targeted.
        path: PathBuf,
    },

    /// The downstream channel was closed before a payload could be sent, which means the pipeline is
    /// shutting down.
    #[error("downstream channel closed")]
    ChannelClosed,

    /// A file extension carried path structure and could not name a downloaded source.
    #[error("invalid file extension: '{value}'")]
    InvalidFileExtension {
        /// The rejected extension value.
        value: String,
    },
}

#[cfg(test)]
mod tests {
    use crate::error::CollectorError;
    use cassiopeia_common::captured_body::CapturedBody;
    use http::StatusCode;
    use url::Url;

    #[test]
    fn a_status_failure_names_the_url_and_the_status() {
        let error = CollectorError::HttpStatus {
            url: Url::parse("https://schemas.example.org/Sensor.json").unwrap(),
            status: StatusCode::NOT_FOUND,
            body: Box::new(CapturedBody::capped(b"no such schema".to_vec(), 2048)),
        };
        let message = error.to_string();

        assert!(message.contains("Sensor.json"));
        assert!(message.contains("404"));
    }
}
