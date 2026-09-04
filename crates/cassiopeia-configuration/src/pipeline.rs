use cassiopeia_common::{memory_profile::MemoryProfile, pipeline_mode::PipelineMode};
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use std::num::NonZeroUsize;

// Both counts are built up from `NonZeroUsize::MIN` so the compiler proves them non-zero without
// an unwrap at the constant.
/// How many records a stage accumulates before passing them on, when the configuration does not say.
const DEFAULT_BATCH_SIZE: NonZeroUsize = NonZeroUsize::MIN.saturating_add(9_999);
/// Whether records are extracted one at a time or across several workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtractionMode {
    /// Extract records across several workers.
    #[default]
    Parallel,

    /// Extract records one at a time, which keeps peak memory down and ordering obvious.
    Sequential,
}

/// How the pipeline moves records between its stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SmartDefault)]
#[serde(default)]
pub struct Pipeline {
    /// How many records a stage accumulates before passing them on.
    #[default(DEFAULT_BATCH_SIZE)]
    pub batch_size: NonZeroUsize,

    /// Whether extraction runs across several workers.
    #[default(ExtractionMode::Parallel)]
    pub extraction_mode: ExtractionMode,

    /// Whether stages hand over batches or single records.
    #[default(PipelineMode::Batch)]
    pub mode: PipelineMode,

    /// How many messages may sit between two stages before the producer waits.
    ///
    /// Omitting the value selects unbounded throughput-oriented channels. Supplying a value opts
    /// into bounded back-pressure without enabling the rest of the low-memory profile.
    #[default(None)]
    pub channel_capacity: Option<NonZeroUsize>,

    /// How many worker threads the run's shared thread pool is built with.
    ///
    /// Omitting the value detects a count from the hardware: the performance-core count on a hybrid
    /// machine, every logical processor otherwise.
    #[default(None)]
    pub worker_threads: Option<NonZeroUsize>,

    /// The memory footprint the run is tuned for.
    #[serde(default)]
    #[default(MemoryProfile::Default)]
    pub memory_profile: MemoryProfile,
}

#[cfg(test)]
mod tests {
    use crate::pipeline::{ExtractionMode, Pipeline};
    use cassiopeia_common::memory_profile::MemoryProfile;
    use std::num::NonZeroUsize;

    #[test]
    fn the_defaults_favour_throughput() {
        let pipeline = Pipeline::default();

        assert_eq!(pipeline.batch_size.get(), 10_000);
        assert_eq!(pipeline.channel_capacity, None);
        assert_eq!(pipeline.worker_threads, None);
        assert_eq!(pipeline.extraction_mode, ExtractionMode::Parallel);
        assert_eq!(pipeline.memory_profile, MemoryProfile::Default);
    }

    #[test]
    fn an_explicit_worker_thread_count_is_read_from_the_file() {
        let pipeline: Pipeline = toml::from_str("worker_threads = 6").unwrap();

        assert_eq!(pipeline.worker_threads.map(NonZeroUsize::get), Some(6));
    }

    #[test]
    fn a_zero_worker_thread_count_is_rejected() {
        assert!(toml::from_str::<Pipeline>("worker_threads = 0").is_err());
    }

    #[test]
    fn reads_a_sequential_low_memory_pipeline() {
        let pipeline: Pipeline = toml::from_str(
            r#"
            batch_size = 100
            extraction_mode = "sequential"
            mode = "single"
            channel_capacity = 2
            memory_profile = "low-memory"
            "#,
        )
        .unwrap();

        assert_eq!(pipeline.extraction_mode, ExtractionMode::Sequential);
        assert_eq!(pipeline.memory_profile, MemoryProfile::LowMemory);
        assert_eq!(pipeline.channel_capacity.unwrap().get(), 2);
    }

    #[test]
    fn a_zero_batch_size_is_rejected() {
        assert!(toml::from_str::<Pipeline>("batch_size = 0").is_err());
    }
}
