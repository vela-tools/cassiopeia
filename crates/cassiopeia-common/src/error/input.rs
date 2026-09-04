use thiserror::Error;
use url::Url;

/// Failures raised while reading a data source declaration.
#[derive(Debug, Error)]
pub enum InputError {
    #[error("The file URL '{url}' does not name a path this platform can open")]
    UnusableFileUrl { url: Url },
}
