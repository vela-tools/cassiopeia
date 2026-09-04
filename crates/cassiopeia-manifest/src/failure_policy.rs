use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// How a run reacts to a failed cycle, and what exit status it leaves behind.
///
/// A fatal error tears the failing cycle down regardless of this policy; the policy governs what the
/// run does next and the process exit code. It applies to a one-shot run and a scheduled run alike:
///
/// - [`FailurePolicy::Abort`] stops at the first failed cycle and exits non-zero, leaving any
///   remaining cycles unrun.
/// - [`FailurePolicy::Continue`] runs every remaining cycle, then exits non-zero because a cycle
///   failed.
/// - [`FailurePolicy::Ignore`] runs every remaining cycle and exits zero, tolerating a failed cycle
///   entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Display, EnumString, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum FailurePolicy {
    /// Stop at the first failed cycle and exit non-zero, leaving any remaining cycles unrun.
    #[default]
    Abort,

    /// Run every remaining cycle, then exit non-zero because a cycle failed.
    Continue,

    /// Run every remaining cycle and exit zero, tolerating a failed cycle entirely.
    Ignore,
}

#[cfg(test)]
mod tests {
    use crate::failure_policy::FailurePolicy;
    use std::str::FromStr;

    #[test]
    fn each_mode_serialises_to_its_kebab_case_wire_form() {
        assert_eq!(serde_json::to_string(&FailurePolicy::Abort).unwrap(), r#""abort""#);
        assert_eq!(serde_json::to_string(&FailurePolicy::Continue).unwrap(), r#""continue""#);
        assert_eq!(serde_json::to_string(&FailurePolicy::Ignore).unwrap(), r#""ignore""#);
    }

    #[test]
    fn each_mode_round_trips_through_from_str() {
        assert_eq!(FailurePolicy::from_str("abort").unwrap(), FailurePolicy::Abort);
        assert_eq!(FailurePolicy::from_str("continue").unwrap(), FailurePolicy::Continue);
        assert_eq!(FailurePolicy::from_str("ignore").unwrap(), FailurePolicy::Ignore);
    }

    #[test]
    fn an_unstated_policy_aborts() {
        assert_eq!(FailurePolicy::default(), FailurePolicy::Abort);
    }
}
