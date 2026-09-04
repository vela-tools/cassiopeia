use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// How strictly a run enforces schema validation before an entity is written.
///
/// A verdict is rendered per entity by the validator; this mode decides what the run does with it.
/// The three modes trade strictness for tolerance of made-up data models, which have no schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Display, EnumString, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ValidationMode {
    /// A schema violation or a missing schema only warns; the entity is still written.
    Warn,

    /// A schema-backed entity that violates its schema aborts the run; a missing schema only warns.
    #[default]
    FailWhenSchema,

    /// Any violation, a broken schema, or a missing schema aborts the run.
    Fail,
}

#[cfg(test)]
mod tests {
    use crate::output::validation_mode::ValidationMode;
    use std::str::FromStr;

    #[test]
    fn the_wire_form_is_kebab_case() {
        assert_eq!(serde_json::to_string(&ValidationMode::Warn).unwrap(), r#""warn""#);
        assert_eq!(serde_json::to_string(&ValidationMode::FailWhenSchema).unwrap(), r#""fail-when-schema""#);
        assert_eq!(serde_json::to_string(&ValidationMode::Fail).unwrap(), r#""fail""#);
    }

    #[test]
    fn the_kebab_form_round_trips_through_from_str() {
        assert_eq!(ValidationMode::from_str("fail-when-schema").unwrap(), ValidationMode::FailWhenSchema);
    }

    #[test]
    fn an_unstated_mode_fails_only_when_a_schema_backs_the_entity() {
        assert_eq!(ValidationMode::default(), ValidationMode::FailWhenSchema);
    }
}
