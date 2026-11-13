use cassiopeia_common::{captured_body::CapturedBody, masked_url::MaskedUrl};
use cassiopeia_sensorthings::{
    paging::{next_link::NextLinkError, page::PageError},
    service_root::ServiceRoot,
};
use http::StatusCode;
use std::{io, path::PathBuf};
use url::Url;

/// The cap on a captured error-response body, which exists to be read by a person.
pub const ERROR_BODY_CAP: usize = 2 * 1024;

/// Errors that can occur during data collection.
#[derive(Debug, thiserror::Error)]
pub enum CollectorError {
    /// The HTTP client could not be built.
    #[error("HTTP client could not be created")]
    ClientInit {
        /// The failure reported by the HTTP layer.
        #[source]
        source: reqwest::Error,
    },

    /// An HTTP request never completed.
    #[error("HTTP request to '{url}' failed")]
    Http {
        /// The transport failure reported by the HTTP layer, stripped of the URL it would quote.
        #[source]
        source: reqwest::Error,
        /// The URL the request targeted, masked. Boxed so a URL does not enlarge every `Result` in the
        /// collector and every pipeline error that wraps one.
        url: Box<MaskedUrl>,
    },

    /// The server answered with a non-success status.
    ///
    /// The body is kept rather than discarded: a 4xx or 5xx answers with the server's own
    /// explanation, and throwing it away leaves a failed download with nothing to act on.
    #[error("'{url}' answered {status}")]
    HttpStatus {
        /// The URL the request targeted, masked. Boxed so a URL does not enlarge every `Result` in the
        /// collector and every pipeline error that wraps one.
        url: Box<MaskedUrl>,
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

    /// The run was being stopped, so collection from a SensorThings server ended before its next
    /// request.
    #[error("Collection from '{root}' stopped because the run is being stopped")]
    Interrupted {
        /// The service root collection read from. Boxed so a root does not enlarge every `Result`
        /// in the collector and every pipeline error that wraps one.
        root: Box<ServiceRoot>,
    },

    /// A page linked back to a page its traversal had already read, so following it would never end.
    #[error("'{link}' links back to a page already read, so it is not followed")]
    LinkLoop {
        /// The link, resolved. Boxed so a URL does not enlarge every `Result` in the collector.
        link: Box<Url>,
    },

    /// A page's `@iot.nextLink` is not followed.
    #[error("The next link of '{page}' is not followed")]
    NextLink {
        /// The page that carries the link. Boxed so a URL does not enlarge every `Result` in the
        /// collector.
        page: Box<Url>,
        /// Why it is not followed.
        source: NextLinkError,
    },

    /// A server answered a request with a body that is no page of a collection.
    #[error("'{url}' answered with no page of a collection")]
    PageUnreadable {
        /// The URL the page was requested from. Boxed so a URL does not enlarge every `Result` in the
        /// collector.
        url: Box<Url>,
        /// What the body is not.
        source: PageError,
    },

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
    use cassiopeia_common::{captured_body::CapturedBody, masked_url::MaskedUrl};
    use http::StatusCode;
    use url::Url;

    #[test]
    fn a_status_failure_names_the_url_and_the_status() {
        let error = CollectorError::HttpStatus {
            url: Box::new(MaskedUrl::plain(Url::parse("https://schemas.example.org/Sensor.json").unwrap())),
            status: StatusCode::NOT_FOUND,
            body: Box::new(CapturedBody::capped(b"no such schema".to_vec(), 2048)),
        };
        let message = error.to_string();

        assert!(message.contains("Sensor.json"));
        assert!(message.contains("404"));
    }
}
