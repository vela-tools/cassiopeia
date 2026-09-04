use cassiopeia_common::format::DataFormat;
use cassiopeia_data_profiler::profile::Profile;
use getset::Getters;
use std::path::PathBuf;

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
    /// The format the collector already knows, when it could infer one.
    format_hint: Option<DataFormat>,
}

/// An in-memory payload: data is stored as raw bytes.
#[derive(Debug, Clone, Getters)]
#[getset(get = "pub")]
pub struct BytesPayload {
    /// The raw collected bytes.
    data: Vec<u8>,
    /// The format the collector already knows, when it could infer one.
    format_hint: Option<DataFormat>,
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
    /// Builds a file payload from its path and optional format hint.
    #[must_use]
    pub const fn new(path: PathBuf, format_hint: Option<DataFormat>) -> FilePayload {
        FilePayload { path, format_hint }
    }

    /// Consumes the payload and returns the owned path.
    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.path
    }
}

impl BytesPayload {
    /// Builds a bytes payload from its raw bytes and optional format hint.
    #[must_use]
    pub const fn new(data: Vec<u8>, format_hint: Option<DataFormat>) -> BytesPayload {
        BytesPayload { data, format_hint }
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
}

#[cfg(test)]
mod tests {
    use crate::payload::{BytesPayload, CollectedPayload, FilePayload};
    use cassiopeia_common::format::DataFormat;
    use std::path::PathBuf;

    #[test]
    fn a_file_payload_exposes_its_format_hint() {
        let payload = CollectedPayload::File(FilePayload::new(PathBuf::from("data.csv"), Some(DataFormat::Csv)));
        assert_eq!(payload.format_hint(), Some(DataFormat::Csv));
    }

    #[test]
    fn a_bytes_payload_without_a_hint_returns_none() {
        let payload = CollectedPayload::Bytes(BytesPayload::new(vec![1, 2, 3], None));
        assert_eq!(payload.format_hint(), None);
    }

    #[test]
    fn into_path_yields_the_owned_path() {
        let payload = FilePayload::new(PathBuf::from("data.csv"), None);
        assert_eq!(payload.into_path(), PathBuf::from("data.csv"));
    }
}
