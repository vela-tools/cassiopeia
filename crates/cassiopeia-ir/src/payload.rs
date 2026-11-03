use cassiopeia_common::{format::DataFormat, input::Input};
use cassiopeia_data_profiler::profile::Profile;
use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
use getset::Getters;
use std::path::PathBuf;
use url::Url;

/// Data collected by the Collector stage, before format detection.
#[derive(Debug, Clone)]
pub enum CollectedPayload {
    /// Large data already on disk (CSV files, downloaded URLs, etc.)
    File(FilePayload),
    /// Small data held in memory as raw bytes.
    Bytes(BytesPayload),
}

/// A file-based payload: data is on disk at the given path.
#[derive(Debug, Clone, Getters)]
#[getset(get = "pub")]
pub struct FilePayload {
    /// The filesystem path the collected data resides at.
    path: PathBuf,
    /// The input the data was collected from: the file itself, or the URL it was downloaded from. A
    /// diagnostic names the data by it, since a download's path is a temporary copy that changes
    /// from one run to the next.
    source: Input,
    /// The format the collector already knows, when it could infer one.
    format_hint: Option<DataFormat>,
    /// What the data's source says about its character encoding.
    encoding_hint: EncodingHint,
}

/// An in-memory payload: data is stored as raw bytes.
#[derive(Debug, Clone, Getters)]
#[getset(get = "pub")]
pub struct BytesPayload {
    /// The raw collected bytes.
    data: Vec<u8>,
    /// The format the collector already knows, when it could infer one.
    format_hint: Option<DataFormat>,
    /// What the data's source says about its character encoding.
    encoding_hint: EncodingHint,
}

/// Data after the Profiler stage: the format is now known.
#[derive(Debug, Clone, Getters)]
#[getset(get = "pub")]
pub struct ProfiledPayload {
    /// The collected payload the profile describes.
    payload: CollectedPayload,
    /// The profile the profiler stage derived for the payload.
    profile: Profile,
}

impl FilePayload {
    /// Builds the payload of a local file read in place, its own source, from its path, optional
    /// format hint, and encoding hint.
    #[must_use]
    pub fn local(path: PathBuf, format_hint: Option<DataFormat>, encoding_hint: EncodingHint) -> FilePayload {
        FilePayload {
            // The payload reads the file at its path and names it by the same path as its source.
            source: Input::Local(path.clone()),
            path,
            format_hint,
            encoding_hint,
        }
    }

    /// Builds the payload of a file downloaded from `url` to `path`, from its optional format hint
    /// and encoding hint.
    #[must_use]
    pub const fn downloaded(path: PathBuf, url: Url, format_hint: Option<DataFormat>, encoding_hint: EncodingHint) -> FilePayload {
        FilePayload {
            path,
            source: Input::Remote(url),
            format_hint,
            encoding_hint,
        }
    }

    /// Consumes the payload and returns the owned path.
    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.path
    }

    /// Consumes the payload and returns the owned path it is read from and the source it is named by.
    #[must_use]
    pub fn into_path_and_source(self) -> (PathBuf, Input) {
        (self.path, self.source)
    }
}

impl BytesPayload {
    /// Builds a bytes payload from its raw bytes, optional format hint, and encoding hint.
    #[must_use]
    pub const fn new(data: Vec<u8>, format_hint: Option<DataFormat>, encoding_hint: EncodingHint) -> BytesPayload {
        BytesPayload {
            data,
            format_hint,
            encoding_hint,
        }
    }
}

impl ProfiledPayload {
    /// Pairs a collected payload with the profile the profiler derived for it.
    #[must_use]
    pub const fn new(payload: CollectedPayload, profile: Profile) -> ProfiledPayload {
        ProfiledPayload { payload, profile }
    }

    /// Consumes the profiled payload and returns the owned collected payload.
    #[must_use]
    pub fn into_payload(self) -> CollectedPayload {
        self.payload
    }

    /// Consumes the profiled payload and returns the owned collected payload and its profile.
    #[must_use]
    pub fn into_parts(self) -> (CollectedPayload, Profile) {
        (self.payload, self.profile)
    }
}

impl CollectedPayload {
    /// Returns the format hint, if any.
    #[must_use]
    pub const fn format_hint(&self) -> Option<DataFormat> {
        match self {
            CollectedPayload::File(f) => f.format_hint,
            CollectedPayload::Bytes(b) => b.format_hint,
        }
    }

    /// Returns what the payload's source says about its character encoding.
    #[must_use]
    pub const fn encoding_hint(&self) -> &EncodingHint {
        match self {
            CollectedPayload::File(f) => &f.encoding_hint,
            CollectedPayload::Bytes(b) => &b.encoding_hint,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::payload::{BytesPayload, CollectedPayload, FilePayload, ProfiledPayload};
    use cassiopeia_common::{format::DataFormat, input::Input};
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
    use mediatype::media_type;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;
    use url::Url;

    #[test]
    fn a_file_payload_exposes_its_format_hint() {
        let payload = CollectedPayload::File(FilePayload::local(PathBuf::from("data.csv"), Some(DataFormat::Csv), EncodingHint::NONE));
        assert_eq!(payload.format_hint(), Some(DataFormat::Csv));
    }

    #[test]
    fn a_bytes_payload_without_a_hint_returns_none() {
        let payload = CollectedPayload::Bytes(BytesPayload::new(vec![1, 2, 3], None, EncodingHint::NONE));
        assert_eq!(payload.format_hint(), None);
    }

    #[test]
    fn a_local_file_payload_is_its_own_source() {
        let payload = FilePayload::local(PathBuf::from("data.csv"), None, EncodingHint::NONE);

        assert_eq!(payload.source(), &Input::Local(PathBuf::from("data.csv")));
    }

    #[test]
    fn a_downloaded_file_payload_is_named_by_its_url() {
        let url = Url::parse("https://example.org/feed.csv").unwrap();
        let payload = FilePayload::downloaded(PathBuf::from("/tmp/input.csv"), url.clone(), None, EncodingHint::NONE);

        assert_eq!(payload.source(), &Input::Remote(url));
        assert_eq!(payload.into_path_and_source().0, PathBuf::from("/tmp/input.csv"));
    }

    #[test]
    fn into_path_yields_the_owned_path() {
        let payload = FilePayload::local(PathBuf::from("data.csv"), None, EncodingHint::NONE);
        assert_eq!(payload.into_path(), PathBuf::from("data.csv"));
    }

    #[test]
    fn into_parts_yields_the_owned_payload_and_its_profile() {
        let payload = CollectedPayload::Bytes(BytesPayload::new(vec![1, 2, 3], None, EncodingHint::NONE));
        let profiled = ProfiledPayload::new(payload, Profile::new(DataFormat::Csv, media_type!(TEXT / CSV), 0.9));

        let (payload, profile) = profiled.into_parts();

        assert!(matches!(payload, CollectedPayload::Bytes(bytes) if bytes.data() == &[1, 2, 3]));
        assert_eq!(*profile.format(), DataFormat::Csv);
    }
}
