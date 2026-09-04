use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// How records move through the pipeline stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum PipelineMode {
    /// Buffered parallel processing.
    #[default]
    Batch,
    /// Immediate per-item processing.
    Single,
}

#[cfg(test)]
mod tests {
    use crate::pipeline_mode::PipelineMode;

    #[test]
    fn processing_is_batched_by_default() {
        assert_eq!(PipelineMode::default(), PipelineMode::Batch);
    }

    #[test]
    fn each_mode_round_trips_through_its_lowercase_token() {
        for (mode, token) in [(PipelineMode::Batch, r#""batch""#), (PipelineMode::Single, r#""single""#)] {
            assert_eq!(serde_json::to_string(&mode).unwrap(), token);
            assert_eq!(serde_json::from_str::<PipelineMode>(token).unwrap(), mode);
        }
    }
}
