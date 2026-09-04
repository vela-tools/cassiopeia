use serde_json::{Map, Value};
use std::str::FromStr;
use thiserror::Error;

/// A single run-level variable supplied on the command line as `KEY=VALUE`.
///
/// Run variables reach every mapping in the run as `{{ vars.<key> }}`. The command line carries only
/// string values; a manifest may declare a variable of any JSON type. A repeated key keeps the last
/// value, resolved when the list is collapsed into a map by [`run_vars_to_map`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunVar {
    /// The variable name, referenced in a mapping as `{{ vars.<key> }}`.
    key: String,
    /// The variable's string value, kept verbatim.
    value: String,
}

impl RunVar {
    /// The variable name.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The variable's string value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl FromStr for RunVar {
    type Err = RunVarError;

    /// Parses a variable written as `KEY=VALUE`, splitting on the first `=`. The key is trimmed; the
    /// value is kept verbatim so a value may itself contain an `=`.
    fn from_str(s: &str) -> Result<RunVar, RunVarError> {
        let (key, value) = s.split_once('=').ok_or(RunVarError::Malformed)?;
        let key = key.trim();
        if key.is_empty() {
            return Err(RunVarError::EmptyKey);
        }
        Ok(RunVar {
            key: key.to_owned(),
            value: value.to_owned(),
        })
    }
}

/// Collapses a list of run variables into a JSON object of name to (string) value, the last value
/// winning on a repeated key.
#[must_use]
pub fn run_vars_to_map(vars: &[RunVar]) -> Map<String, Value> {
    let mut map = Map::new();
    for var in vars {
        map.insert(var.key.clone(), Value::String(var.value.clone()));
    }
    map
}

/// The reason a run variable could not be read from its command-line form.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RunVarError {
    /// The variable was not written in `KEY=VALUE` form.
    #[error("run variable must be written as 'KEY=VALUE'")]
    Malformed,
    /// The variable name is empty.
    #[error("run variable key must not be empty")]
    EmptyKey,
}

#[cfg(test)]
mod tests {
    use crate::run_var::{RunVar, RunVarError, run_vars_to_map};
    use serde_json::json;
    use std::str::FromStr;

    #[test]
    fn a_key_value_pair_parses() {
        let var = RunVar::from_str("valid_from=2026-08-04T16:00:00Z").unwrap();

        assert_eq!(var.key(), "valid_from");
        assert_eq!(var.value(), "2026-08-04T16:00:00Z");
    }

    #[test]
    fn the_value_may_contain_further_equals_signs() {
        let var = RunVar::from_str("query=a=b").unwrap();

        assert_eq!(var.key(), "query");
        assert_eq!(var.value(), "a=b");
    }

    #[test]
    fn the_key_is_trimmed() {
        let var = RunVar::from_str("  spaced  =value").unwrap();

        assert_eq!(var.key(), "spaced");
    }

    #[test]
    fn a_pair_without_an_equals_sign_is_malformed() {
        assert_eq!(RunVar::from_str("no-equals-here"), Err(RunVarError::Malformed));
    }

    #[test]
    fn an_empty_key_is_rejected() {
        assert_eq!(RunVar::from_str("=value"), Err(RunVarError::EmptyKey));
    }

    #[test]
    fn a_repeated_key_keeps_the_last_value() {
        let vars = vec![RunVar::from_str("a=1").unwrap(), RunVar::from_str("a=2").unwrap()];

        let map = run_vars_to_map(&vars);

        assert_eq!(map.get("a"), Some(&json!("2")));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn an_empty_list_yields_an_empty_map() {
        assert!(run_vars_to_map(&[]).is_empty());
    }
}
