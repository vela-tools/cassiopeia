use strum::{Display, EnumCount, EnumIter};

/// Why a run, or one of its scheduled cycles, did not complete as asked.
///
/// These name failures of the composition itself rather than of one pipeline stage: a mapping that
/// will not load, a store that will not answer, a worker that panicked.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum RunCode {
    /// The run failed for a reason no more specific code names.
    Failed,
    /// One scheduled cycle failed while the failure policy let the schedule carry on.
    CycleFailed,
    /// One cycle failed and is being retried.
    CycleRetrying,
    /// A mapping file could not be read or parsed.
    MappingUnusable,
    /// Fragment resolution or one of its backing stores failed.
    ResolutionFailed,
    /// The destination could not accept the run's entities.
    OutputFailed,
    /// A stage's worker thread panicked.
    StagePanic,
    /// The validation report could not be serialized or written.
    ReportUnwritable,
    /// The run's settings contradict each other.
    Misconfigured,
}

#[cfg(test)]
mod tests {
    use crate::code::run_code::RunCode;

    #[test]
    fn a_run_code_renders_a_kebab_case_token() {
        assert_eq!(RunCode::CycleRetrying.to_string(), "cycle-retrying");
        assert_eq!(RunCode::StagePanic.to_string(), "stage-panic");
    }
}
