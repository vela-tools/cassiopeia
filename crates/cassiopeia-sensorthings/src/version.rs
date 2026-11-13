use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// A SensorThings API version Cassiopeia reads, written as the last path segment of a service root
/// writes it: `"v" major "." minor` (OGC 18-088 section 9.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum StaVersion {
    /// SensorThings API Part 1: Sensing, version 1.0 (OGC 15-078r6).
    #[serde(rename = "v1.0")]
    #[strum(serialize = "v1.0")]
    V1_0,

    /// SensorThings API Part 1: Sensing, version 1.1 (OGC 18-088).
    #[serde(rename = "v1.1")]
    #[strum(serialize = "v1.1")]
    V1_1,
}

#[cfg(test)]
mod tests {
    use crate::version::StaVersion;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_version_is_written_as_its_path_segment() {
        assert_eq!(StaVersion::V1_0.to_string(), "v1.0");
        assert_eq!(StaVersion::V1_1.to_string(), "v1.1");
    }

    #[test]
    fn a_version_is_read_from_its_path_segment() {
        assert_eq!("v1.1".parse::<StaVersion>().unwrap(), StaVersion::V1_1);
        assert!("v2.0".parse::<StaVersion>().is_err());
    }

    #[test]
    fn a_version_serializes_as_its_path_segment() {
        assert_eq!(serde_json::to_string(&StaVersion::V1_0).unwrap(), r#""v1.0""#);
        assert_eq!(serde_json::from_str::<StaVersion>(r#""v1.1""#).unwrap(), StaVersion::V1_1);
    }
}
