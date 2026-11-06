use thiserror::Error;

/// Why a remote `@context` document could not be loaded.
#[derive(Debug, Error)]
pub enum ContextLoadError {
    /// The HTTP client could not be built.
    #[error("The HTTP client could not be built")]
    Client(#[source] reqwest::Error),

    /// The request did not complete: a connection, TLS or timeout failure.
    #[error("The request failed")]
    Request(#[source] reqwest::Error),

    /// The server answered with a status other than success.
    #[error("The server answered {status}")]
    Status {
        /// The status the server answered with.
        status: reqwest::StatusCode,
    },

    /// The response body is not JSON.
    #[error("The document is not JSON")]
    Body(#[source] reqwest::Error),
}
