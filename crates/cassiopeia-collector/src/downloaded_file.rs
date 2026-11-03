use getset::Getters;
use mediatype::MediaTypeBuf;
use std::path::PathBuf;
use url::Url;

/// A remote resource written to disk, with the URL it was served from and the media type its server
/// declared for it.
#[derive(Debug, Getters)]
#[getset(get = "pub")]
pub struct DownloadedFile {
    /// Where the response body was written.
    path: PathBuf,
    /// The URL the body was served from, after every redirect: the server whose domain and
    /// `Content-Type` describe the body, which can differ from the URL requested.
    url: Url,
    /// The `Content-Type` the server answered with, when it sent a parseable one.
    content_type: Option<MediaTypeBuf>,
}

impl DownloadedFile {
    /// Pairs a written download with the URL it was served from and the media type its server
    /// declared.
    #[must_use]
    pub const fn new(path: PathBuf, url: Url, content_type: Option<MediaTypeBuf>) -> DownloadedFile {
        DownloadedFile { path, url, content_type }
    }

    /// Consumes the download and returns the owned path it was written to.
    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.path
    }
}
