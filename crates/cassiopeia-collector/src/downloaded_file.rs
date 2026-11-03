use cassiopeia_common::masked_url::MaskedUrl;
use getset::Getters;
use mediatype::MediaTypeBuf;
use std::path::PathBuf;

/// A remote resource written to disk, with the URL it was served from and the media type its server
/// declared for it.
#[derive(Debug, Getters)]
#[getset(get = "pub")]
pub struct DownloadedFile {
    /// Where the response body was written.
    path: PathBuf,
    /// The URL the body was served from, after every redirect: the server whose domain and
    /// `Content-Type` describe the body, which can differ from the URL requested. Masked as the
    /// requested URL is, since a redirect can carry its secrets along.
    url: MaskedUrl,
    /// The `Content-Type` the server answered with, when it sent a parseable one.
    content_type: Option<MediaTypeBuf>,
}

impl DownloadedFile {
    /// Pairs a written download with the URL it was served from and the media type its server
    /// declared.
    #[must_use]
    pub const fn new(path: PathBuf, url: MaskedUrl, content_type: Option<MediaTypeBuf>) -> DownloadedFile {
        DownloadedFile { path, url, content_type }
    }

    /// Consumes the download and returns the owned path it was written to.
    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.path
    }
}

#[cfg(test)]
mod tests {
    use crate::downloaded_file::DownloadedFile;
    use cassiopeia_common::masked_url::MaskedUrl;
    use mediatype::MediaTypeBuf;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;
    use url::Url;

    fn plain(url: &str) -> MaskedUrl {
        MaskedUrl::plain(Url::parse(url).unwrap())
    }

    #[test]
    fn a_download_exposes_its_path_url_and_declared_media_type() {
        let url = MaskedUrl::plain(Url::parse("https://example.org/feed.csv").unwrap());
        let content_type = "text/csv; charset=utf-8".parse::<MediaTypeBuf>().unwrap();
        let file = DownloadedFile::new(PathBuf::from("/tmp/input.csv"), url.clone(), Some(content_type.clone()));

        assert_eq!(file.path(), &PathBuf::from("/tmp/input.csv"));
        assert_eq!(file.url(), &url);
        assert_eq!(file.content_type(), &Some(content_type));
    }

    #[test]
    fn a_download_without_a_media_type_declares_none() {
        let file = DownloadedFile::new(PathBuf::from("/tmp/input.csv"), plain("https://example.org/feed.csv"), None);

        assert_eq!(file.content_type(), &None);
    }

    #[test]
    fn into_path_yields_the_owned_path() {
        let file = DownloadedFile::new(PathBuf::from("/tmp/input.csv"), plain("https://example.org/feed.csv"), None);

        assert_eq!(file.into_path(), PathBuf::from("/tmp/input.csv"));
    }
}
