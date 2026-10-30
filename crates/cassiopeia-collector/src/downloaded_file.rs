use getset::Getters;
use mediatype::MediaTypeBuf;
use std::path::PathBuf;

/// A remote resource written to disk, with the media type its server declared for it.
#[derive(Debug, Getters)]
#[getset(get = "pub")]
pub struct DownloadedFile {
    /// Where the response body was written.
    path: PathBuf,
    /// The `Content-Type` the server answered with, when it sent a parseable one.
    content_type: Option<MediaTypeBuf>,
}

impl DownloadedFile {
    /// Pairs a written download with the media type its server declared.
    #[must_use]
    pub const fn new(path: PathBuf, content_type: Option<MediaTypeBuf>) -> DownloadedFile {
        DownloadedFile { path, content_type }
    }

    /// Consumes the download and returns the owned path it was written to.
    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.path
    }
}
