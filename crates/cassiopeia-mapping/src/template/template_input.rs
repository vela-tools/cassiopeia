use crate::template::run_variables::RunVariables;
use serde_json::Value as JsonValue;

/// What one template reads: a source record, and the run variables of the input it came from.
///
/// The two are held apart so that the record stays exactly what the source gave. A path that starts
/// at `vars` reads the variables; every other path, and `this`, reads the record.
#[derive(Debug, Clone, Copy)]
pub struct TemplateInput<'a> {
    /// The source record.
    record: &'a JsonValue,
    /// The run variables the record is read with.
    variables: &'a RunVariables,
}

impl<'a> TemplateInput<'a> {
    /// Pairs a source record with the run variables it is read with.
    #[must_use]
    pub const fn new(record: &'a JsonValue, variables: &'a RunVariables) -> TemplateInput<'a> {
        TemplateInput { record, variables }
    }

    /// The source record.
    #[must_use]
    pub const fn record(&self) -> &'a JsonValue {
        self.record
    }

    /// The run variables the record is read with.
    #[must_use]
    pub const fn variables(&self) -> &'a RunVariables {
        self.variables
    }

    /// Reads another record with the same run variables, such as a nested object an attribute builds
    /// its members from, or the token a synthetic entity is minted from.
    #[must_use]
    pub const fn with_record(self, record: &'a JsonValue) -> TemplateInput<'a> {
        TemplateInput {
            record,
            variables: self.variables,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::template::{run_variables::RunVariables, template_input::TemplateInput};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn another_record_is_read_with_the_same_variables() {
        let variables = RunVariables::none();
        let record = json!({"id": 1});
        let nested = json!({"street": "Slovenska"});

        let input = TemplateInput::new(&record, &variables).with_record(&nested);

        assert_eq!(input.record(), &nested);
        assert_eq!(input.variables().address(), variables.address());
    }
}
