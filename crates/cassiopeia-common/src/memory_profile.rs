use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Coarse-grained memory footprint profile.
///
/// `LowMemory` bounds in-flight memory by shrinking batch sizes and channel capacities and by
/// routing resolver state to a disk-backed store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryProfile {
    /// Tuned for throughput; uses in-memory resolver state.
    #[default]
    #[value(name = "default")]
    Default,
    /// Tuned to minimize peak RSS; disk-backed resolver state and smaller batches.
    #[value(name = "low-memory")]
    #[serde(alias = "lowMemory", alias = "low_memory")]
    LowMemory,
}

#[cfg(test)]
mod tests {
    use crate::memory_profile::MemoryProfile;

    #[test]
    fn throughput_tuning_is_the_default() {
        assert_eq!(MemoryProfile::default(), MemoryProfile::Default);
    }

    #[test]
    fn the_low_memory_profile_is_accepted_in_every_spelling() {
        for token in [r#""low-memory""#, r#""lowMemory""#, r#""low_memory""#] {
            assert_eq!(serde_json::from_str::<MemoryProfile>(token).unwrap(), MemoryProfile::LowMemory);
        }
    }

    #[test]
    fn the_low_memory_profile_serializes_in_kebab_case() {
        assert_eq!(serde_json::to_string(&MemoryProfile::LowMemory).unwrap(), r#""low-memory""#);
    }
}
