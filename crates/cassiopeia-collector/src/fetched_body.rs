use crate::downloaded_file::DownloadedFile;
use cassiopeia_common::{captured_body::CapturedBody, masked_url::MaskedUrl};
use getset::Getters;
use mediatype::MediaTypeBuf;
use std::path::PathBuf;

/// A resource a server served, read in full, with the URL it was served from and the media type the
/// server declared for it.
#[derive(Debug, Getters)]
pub struct FetchedBody {
    /// The body, captured whole.
    #[getset(skip)]
    body: CapturedBody,
    /// The URL the body was served from, after every redirect, masked as the requested URL is.
    #[getset(get = "pub")]
    served_from: MaskedUrl,
    /// The `Content-Type` the server answered with, when it sent a parseable one.
    #[getset(get = "pub")]
    content_type: Option<MediaTypeBuf>,
}

impl FetchedBody {
    /// Pairs a body captured whole with the URL it was served from and its declared media type.
    pub(crate) const fn new(body: CapturedBody, served_from: MaskedUrl, content_type: Option<MediaTypeBuf>) -> FetchedBody {
        FetchedBody {
            body,
            served_from,
            content_type,
        }
    }

    /// The body's bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.body.bytes()
    }

    /// Consumes the body once it is written to `path`, and returns the download that names it.
    pub(crate) fn into_download(self, path: PathBuf) -> DownloadedFile {
        DownloadedFile::new(path, self.served_from, self.content_type)
    }
}
