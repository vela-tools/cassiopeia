use crate::schema_violation::SchemaViolation;
use jsonschema::Validator as JsonSchemaValidator;
use serde_json::Value;
use std::fmt::{Display, Formatter, Result as FmtResult};

/// The number of violations retained per entity, so a wildly nonconforming entity cannot produce an
/// unbounded list.
const MAX_VIOLATIONS_PER_ENTITY: usize = 10;

/// Whether the retained violations are all of them.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ViolationCount {
    /// Every violation was kept, and there were this many.
    Complete {
        /// How many violations the entity had.
        total: usize,
    },
    /// The list was cut at the cap, and at least one more existed.
    Truncated {
        /// How many were kept.
        kept: usize,
    },
}

/// The violations kept for one entity, and whether they are all of them.
///
/// Counting the true total would defeat the cap: an `additionalProperties` failure on a
/// five-hundred-attribute entity constructs five hundred errors, each with its own message. Pulling
/// exactly one element past the cap is O(1) and still honest: it is the difference between saying
/// "10 violations" when there were two hundred and saying "10 or more".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaViolations {
    /// The violations kept, in the order the engine raised them.
    violations: Vec<SchemaViolation>,
    /// Whether anything was left out.
    count: ViolationCount,
}

impl SchemaViolations {
    /// Collects the violations one known-invalid entity has, capped.
    #[must_use]
    pub fn collect(validator: &JsonSchemaValidator, entity: &Value) -> SchemaViolations {
        let mut violations: Vec<SchemaViolation> = validator
            .iter_errors(entity)
            .take(MAX_VIOLATIONS_PER_ENTITY + 1)
            .map(SchemaViolation::from_error)
            .collect();

        if violations.len() > MAX_VIOLATIONS_PER_ENTITY {
            violations.truncate(MAX_VIOLATIONS_PER_ENTITY);
            return SchemaViolations {
                violations,
                count: ViolationCount::Truncated {
                    kept: MAX_VIOLATIONS_PER_ENTITY,
                },
            };
        }

        let total = violations.len();
        SchemaViolations {
            violations,
            count: ViolationCount::Complete { total },
        }
    }

    /// The violations kept.
    #[must_use]
    pub fn violations(&self) -> &[SchemaViolation] {
        &self.violations
    }

    /// Whether the kept violations are all of them.
    #[must_use]
    pub const fn count(&self) -> ViolationCount {
        self.count
    }

    /// The first violation, which is the one a one-line summary names.
    #[must_use]
    pub fn first(&self) -> Option<&SchemaViolation> {
        self.violations.first()
    }
}

impl Display for SchemaViolations {
    /// Renders the list on one line: how many there were, and what the first one said.
    ///
    /// One line rather than a debug dump, because this reaches a terminal through an error message
    /// and the structured detail is available to whoever wants it through [`Self::violations`].
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        match self.count {
            ViolationCount::Complete { total } => write!(formatter, "{total} violation(s)")?,
            ViolationCount::Truncated { kept } => write!(formatter, "{kept}+ violation(s)")?,
        }
        match self.first() {
            Some(first) => write!(formatter, ", first at '{}': {}", first.instance_path(), first.message()),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema_violations::{MAX_VIOLATIONS_PER_ENTITY, SchemaViolations, ViolationCount};
    use jsonschema::Validator;
    use serde_json::{Map, Value, json};

    fn collect(schema: &Value, instance: &Value) -> SchemaViolations {
        let validator = Validator::new(schema).expect("a compilable schema");
        SchemaViolations::collect(&validator, instance)
    }

    /// An object with `count` members none of which the schema admits.
    fn wide(count: usize) -> Value {
        let mut object = Map::new();
        for index in 0..count {
            object.insert(format!("extra{index}"), json!(1));
        }
        Value::Object(object)
    }

    #[test]
    fn a_list_within_the_cap_reports_its_true_total() {
        let violations = collect(&json!({"type": "object", "required": ["a", "b", "c"]}), &json!({}));

        assert_eq!(violations.count(), ViolationCount::Complete { total: 3 });
        assert_eq!(violations.violations().len(), 3);
    }

    #[test]
    fn a_list_past_the_cap_reports_that_it_was_cut() {
        let schema = json!({"type": "object", "properties": {}, "additionalProperties": {"type": "string"}});

        let violations = collect(&schema, &wide(MAX_VIOLATIONS_PER_ENTITY + 5));

        assert_eq!(
            violations.count(),
            ViolationCount::Truncated {
                kept: MAX_VIOLATIONS_PER_ENTITY
            }
        );
        assert_eq!(violations.violations().len(), MAX_VIOLATIONS_PER_ENTITY);
    }

    #[test]
    fn a_summary_is_one_line_naming_the_first_violation() {
        let violations = collect(&json!({"type": "object", "required": ["temperature"]}), &json!({}));
        let summary = violations.to_string();

        assert!(!summary.contains('\n'));
        assert!(summary.starts_with("1 violation(s)"));
        assert!(summary.contains("temperature"));
    }

    #[test]
    fn a_truncated_summary_says_the_count_is_a_floor() {
        let schema = json!({"type": "object", "properties": {}, "additionalProperties": {"type": "string"}});

        let summary = collect(&schema, &wide(MAX_VIOLATIONS_PER_ENTITY + 5)).to_string();

        assert!(summary.starts_with("10+ violation(s)"));
    }
}
