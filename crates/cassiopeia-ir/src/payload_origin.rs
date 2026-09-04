use crate::payload::CollectedPayload;
use derive_more::Display;
use std::path::PathBuf;

/// Where a payload came from, for a diagnostic that has to name it.
///
/// A failure on a payload is far less useful without saying which payload: "no ingestor for format
/// GRIB" leaves a run with a dozen inputs nothing to look at. An in-memory payload has no path to
/// give, and saying so is more honest than inventing one.
#[derive(Clone, Debug, Display, Eq, PartialEq)]
pub enum PayloadOrigin {
    /// The payload was read from a file.
    #[display("{}", _0.display())]
    File(PathBuf),
    /// The payload was held in memory and never had a path.
    #[display("an in-memory payload")]
    Memory,
}

impl PayloadOrigin {
    /// Names where a collected payload came from.
    #[must_use]
    pub fn of(payload: &CollectedPayload) -> PayloadOrigin {
        match payload {
            // The origin outlives the borrowed payload, so it owns the path.
            CollectedPayload::File(file) => PayloadOrigin::File(file.path().clone()),
            CollectedPayload::Bytes(_) => PayloadOrigin::Memory,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        payload::{BytesPayload, CollectedPayload, FilePayload},
        payload_origin::PayloadOrigin,
    };
    use std::path::PathBuf;

    #[test]
    fn a_file_payload_names_its_path() {
        let payload = CollectedPayload::File(FilePayload::new(PathBuf::from("/data/stations.csv"), None));

        assert_eq!(PayloadOrigin::of(&payload).to_string(), "/data/stations.csv");
    }

    #[test]
    fn an_in_memory_payload_says_so_rather_than_naming_a_path() {
        let payload = CollectedPayload::Bytes(BytesPayload::new(b"id,name".to_vec(), None));

        assert_eq!(PayloadOrigin::of(&payload), PayloadOrigin::Memory);
        assert_eq!(PayloadOrigin::of(&payload).to_string(), "an in-memory payload");
    }
}
