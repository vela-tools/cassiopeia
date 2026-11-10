use serde_json::{Map, Value as JsonValue};
use std::sync::Arc;

/// The name a template reads the run variables through, as `{{ vars.NAME }}`.
pub(crate) const VARIABLES: &str = "vars";

/// The run variables one input's records are read with: what a mapping reads as `{{ vars.NAME }}`.
///
/// They travel beside each record rather than inside it, so a template that reaches the whole record,
/// `{{ context }}` or `this`, sees only the fields the source holds, and a source field that happens
/// to be called `vars` is not overwritten. Every record of an input shares one map, so a clone only
/// bumps a reference count.
#[derive(Debug, Clone, PartialEq)]
pub struct RunVariables(Arc<JsonValue>);

impl RunVariables {
    /// Wraps the variables an input's records are read with.
    #[must_use]
    pub fn new(variables: Map<String, JsonValue>) -> RunVariables {
        RunVariables(Arc::new(JsonValue::Object(variables)))
    }

    /// No variables at all, for records whose input sets none.
    #[must_use]
    pub fn none() -> RunVariables {
        RunVariables::new(Map::new())
    }

    /// The variables as one JSON object, which `{{ vars }}` reads whole.
    #[must_use]
    pub fn as_value(&self) -> &JsonValue {
        &self.0
    }

    /// The address of the shared map, which identifies this set of variables, and every clone of it,
    /// for as long as one of them is alive.
    #[must_use]
    pub fn address(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}

#[cfg(test)]
mod tests {
    use crate::template::run_variables::RunVariables;
    use pretty_assertions::assert_eq;
    use serde_json::{Map, json};

    #[test]
    fn the_variables_read_as_one_object() {
        let mut variables = Map::new();
        variables.insert("network".to_string(), json!("bikes"));

        assert_eq!(RunVariables::new(variables).as_value(), &json!({"network": "bikes"}));
        assert_eq!(RunVariables::none().as_value(), &json!({}));
    }

    #[test]
    fn a_clone_shares_the_address_of_its_original() {
        let variables = RunVariables::none();

        assert_eq!(variables.clone().address(), variables.address());
        assert_ne!(RunVariables::none().address(), variables.address());
    }

    #[test]
    fn two_sets_holding_the_same_variables_are_equal() {
        assert_eq!(RunVariables::none(), RunVariables::none());
    }
}
