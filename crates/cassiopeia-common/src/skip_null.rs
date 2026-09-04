use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Whether null-valued attributes take part in serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum NgsiLdSkipNull {
    /// Include null-valued attributes in output.
    Include,
    /// Omit null-valued attributes from output.
    #[default]
    Skip,
}

#[cfg(test)]
mod tests {
    use crate::skip_null::NgsiLdSkipNull;

    #[test]
    fn every_choice_round_trips_through_its_lowercase_token() {
        for (skip_null, token) in [(NgsiLdSkipNull::Include, r#""include""#), (NgsiLdSkipNull::Skip, r#""skip""#)] {
            assert_eq!(serde_json::to_string(&skip_null).unwrap(), token);
            assert_eq!(serde_json::from_str::<NgsiLdSkipNull>(token).unwrap(), skip_null);
        }
    }

    #[test]
    fn nulls_are_skipped_by_default() {
        assert_eq!(NgsiLdSkipNull::default(), NgsiLdSkipNull::Skip);
    }
}
