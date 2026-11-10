use crate::template::{resolver::RECORD_VARIABLE, run_variables::VARIABLES, template_input::TemplateInput};
use serde_json::Value as JsonValue;
use std::fmt;

/// The field reference whose lone use as an attribute source takes the whole record as the value.
const WHOLE_RECORD: &str = "context";

/// What a [`FieldPath`] is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PathRoot {
    /// The source record.
    Record,
    /// The run variables, for a path written from `vars`.
    Variables,
}

/// A dot-separated path naming a source field, such as `properties.tipo`, or a run variable, such as
/// `vars.network`.
///
/// A newtype rather than a bare `String` so a field reference cannot be confused with literal
/// template text or an unrelated string; it owns the navigation logic that reads the value it names
/// out of a [`TemplateInput`]. A path written from `vars` reads the run variables, unless it was
/// reached through `this`, which is always the record.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldPath {
    /// What the path is read from.
    root: PathRoot,
    /// The path as written, its `vars` head included when it reads the run variables.
    written: String,
}

impl FieldPath {
    /// Wraps a dot-separated path written as a bare reference, such as `properties.name` or
    /// `vars.network`: one that starts at `vars` reads the run variables, any other the record.
    #[must_use]
    pub fn new(path: impl Into<String>) -> FieldPath {
        let written = path.into();
        let head = written.split_once('.').map_or(written.as_str(), |(head, _rest)| head);
        let root = if head == VARIABLES { PathRoot::Variables } else { PathRoot::Record };
        FieldPath { root, written }
    }

    /// Wraps a dot-separated path reached through `this`, which always names a record field, even
    /// one called `vars`.
    #[must_use]
    pub fn in_record(path: impl Into<String>) -> FieldPath {
        FieldPath {
            root: PathRoot::Record,
            written: path.into(),
        }
    }

    /// The path as written, its `vars` head included when it reads the run variables.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.written
    }

    /// The top-level record key the path starts at, or `None` when it reads the run variables.
    #[must_use]
    pub fn record_key(&self) -> Option<&str> {
        match self.root {
            PathRoot::Record => Some(self.written.split_once('.').map_or(self.written.as_str(), |(head, _rest)| head)),
            PathRoot::Variables => None,
        }
    }

    /// Whether this is the reference an attribute source writes, alone, to take the whole record as
    /// its value rather than one field of it.
    #[must_use]
    pub fn is_whole_record(&self) -> bool {
        self.root == PathRoot::Record && self.written == WHOLE_RECORD
    }

    /// Reads the value this path names out of a template input.
    ///
    /// A missing key, or a path that runs through a non-object, yields [`JsonValue::Null`] rather
    /// than an error: absence of a source field is normal for sparse records and is handled
    /// downstream by null-skipping rather than by failing the record.
    #[must_use]
    pub fn read(&self, input: TemplateInput<'_>) -> JsonValue {
        self.lookup(input).cloned().unwrap_or(JsonValue::Null)
    }

    /// The value this path names in a template input, or `None` when it does not have it: a key is
    /// missing, or the path runs through a non-object.
    #[must_use]
    pub fn lookup<'a>(&self, input: TemplateInput<'a>) -> Option<&'a JsonValue> {
        let (origin, skipped) = self.origin(input);
        self.written
            .split('.')
            .skip(skipped)
            .try_fold(origin, |current, segment| current.as_object()?.get(segment))
    }

    /// The shortest leading part of this path that a template input does not have, or `None` when it
    /// has the whole path.
    ///
    /// For `address.street.name` over a record whose `address` has no `street`, that is
    /// `address.street`. A segment beneath a value that is not an object, null included, is one the
    /// input does not have.
    #[must_use]
    pub fn missing_prefix(&self, input: TemplateInput<'_>) -> Option<FieldPath> {
        let (mut current, skipped) = self.origin(input);
        for (end, segment) in self.segment_ends().skip(skipped) {
            match current.as_object().and_then(|fields| fields.get(segment)) {
                Some(value) => current = value,
                None => {
                    return Some(FieldPath {
                        root: self.root,
                        written: self.written[..end].to_string(),
                    });
                }
            }
        }
        None
    }

    /// The last segment of the path: `name` in `address.street.name`.
    #[must_use]
    pub fn last_segment(&self) -> &str {
        self.written.rsplit_once('.').map_or(self.written.as_str(), |(_rest, last)| last)
    }

    /// The value the path is read from, and how many of its leading segments name that value rather
    /// than a key beneath it: the `vars` head of a path that reads the run variables.
    fn origin<'a>(&self, input: TemplateInput<'a>) -> (&'a JsonValue, usize) {
        match self.root {
            PathRoot::Record => (input.record(), 0),
            PathRoot::Variables => (input.variables().as_value(), 1),
        }
    }

    /// Each segment of the path with the byte offset just past it.
    fn segment_ends(&self) -> impl Iterator<Item = (usize, &str)> {
        self.written.split('.').scan(0, |start, segment| {
            let end = *start + segment.len();
            *start = end + 1;
            Some((end, segment))
        })
    }
}

/// Displays the path the way a template reads it back, which is how a failure suggests guarding or
/// defaulting it: a record field whose path starts at `vars` is written through `this`, since a bare
/// `vars` reads the run variables. A derive cannot choose that prefix by the root, so this is written
/// by hand.
impl fmt::Display for FieldPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shadowed = self.root == PathRoot::Record && self.written.split('.').next() == Some(VARIABLES);
        if shadowed {
            write!(formatter, "{RECORD_VARIABLE}.{}", self.written)
        } else {
            formatter.write_str(&self.written)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::template::{field_path::FieldPath, run_variables::RunVariables, template_input::TemplateInput};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value as JsonValue, json};

    /// The variables `network` and `districts` that the run-variable tests read.
    fn variables() -> RunVariables {
        let mut variables = Map::new();
        variables.insert("network".to_string(), json!("bikes"));
        variables.insert("districts".to_string(), json!({"first": "Center"}));
        RunVariables::new(variables)
    }

    #[test]
    fn reads_a_top_level_key() {
        let variables = RunVariables::none();

        assert_eq!(FieldPath::new("id").read(TemplateInput::new(&json!({"id": 7}), &variables)), json!(7));
    }

    #[test]
    fn walks_a_nested_path() {
        let variables = RunVariables::none();
        let data = json!({"properties": {"tipo": "sensor"}});

        assert_eq!(FieldPath::new("properties.tipo").read(TemplateInput::new(&data, &variables)), json!("sensor"));
    }

    #[test]
    fn a_missing_key_reads_as_null() {
        let variables = RunVariables::none();

        assert_eq!(FieldPath::new("name").read(TemplateInput::new(&json!({"id": 7}), &variables)), JsonValue::Null);
    }

    #[test]
    fn a_path_through_a_non_object_reads_as_null() {
        let variables = RunVariables::none();

        assert_eq!(
            FieldPath::new("id.nested").read(TemplateInput::new(&json!({"id": 7}), &variables)),
            JsonValue::Null
        );
    }

    #[test]
    fn a_nested_object_is_read_whole() {
        let variables = RunVariables::none();
        let data = json!({"address": {"city": "Ljubljana"}});

        assert_eq!(
            FieldPath::new("address").read(TemplateInput::new(&data, &variables)),
            json!({"city": "Ljubljana"})
        );
    }

    #[test]
    fn a_path_from_vars_reads_the_run_variables_and_not_the_record() {
        let variables = variables();
        let data = json!({"vars": {"network": "from the source"}});
        let input = TemplateInput::new(&data, &variables);

        assert_eq!(FieldPath::new("vars.network").read(input), json!("bikes"));
        assert_eq!(FieldPath::new("vars.districts.first").read(input), json!("Center"));
        assert_eq!(&FieldPath::new("vars").read(input), variables.as_value());
    }

    #[test]
    fn a_path_through_this_reads_the_record_even_from_vars() {
        let variables = variables();
        let data = json!({"vars": {"network": "from the source"}});

        assert_eq!(
            FieldPath::in_record("vars.network").read(TemplateInput::new(&data, &variables)),
            json!("from the source")
        );
    }

    #[test]
    fn a_variable_no_level_sets_reads_as_null() {
        let variables = RunVariables::none();

        assert_eq!(FieldPath::new("vars.network").read(TemplateInput::new(&json!({}), &variables)), JsonValue::Null);
    }

    #[test]
    fn the_record_key_is_the_first_segment_and_a_run_variable_has_none() {
        assert_eq!(FieldPath::new("properties.tipo").record_key(), Some("properties"));
        assert_eq!(FieldPath::new("id").record_key(), Some("id"));
        assert_eq!(FieldPath::new("vars.network").record_key(), None);
        assert_eq!(FieldPath::in_record("vars").record_key(), Some("vars"));
    }

    #[test]
    fn only_the_context_reference_is_the_whole_record() {
        assert!(FieldPath::new("context").is_whole_record());
        assert!(!FieldPath::new("context.id").is_whole_record());
    }

    #[test]
    fn a_present_value_is_looked_up_and_a_missing_one_is_none() {
        let variables = RunVariables::none();
        let data = json!({"address": {"city": null}, "id": 7});
        let input = TemplateInput::new(&data, &variables);

        assert_eq!(FieldPath::new("address.city").lookup(input), Some(&JsonValue::Null));
        assert_eq!(FieldPath::new("address.street").lookup(input), None);
        assert_eq!(FieldPath::new("id.nested").lookup(input), None);
    }

    #[test]
    fn the_missing_prefix_ends_at_the_first_segment_the_record_lacks() {
        let variables = RunVariables::none();
        let data = json!({"address": {"city": {}}, "id": 7, "gone": null});
        let input = TemplateInput::new(&data, &variables);

        assert_eq!(
            FieldPath::new("address.street.name").missing_prefix(input),
            Some(FieldPath::new("address.street"))
        );
        assert_eq!(
            FieldPath::new("address.city.name").missing_prefix(input),
            Some(FieldPath::new("address.city.name"))
        );
        assert_eq!(FieldPath::new("name").missing_prefix(input), Some(FieldPath::new("name")));
    }

    #[test]
    fn the_missing_prefix_of_a_run_variable_is_read_beneath_vars() {
        let variables = variables();
        let data = json!({});
        let input = TemplateInput::new(&data, &variables);

        assert_eq!(FieldPath::new("vars.region").missing_prefix(input), Some(FieldPath::new("vars.region")));
        assert_eq!(
            FieldPath::new("vars.districts.last").missing_prefix(input),
            Some(FieldPath::new("vars.districts.last"))
        );
        assert_eq!(FieldPath::new("vars.network").missing_prefix(input), None);
        assert_eq!(FieldPath::new("vars").missing_prefix(input), None);
    }

    #[test]
    fn a_segment_beneath_a_non_object_is_missing() {
        let variables = RunVariables::none();
        let data = json!({"id": 7, "gone": null});
        let input = TemplateInput::new(&data, &variables);

        assert_eq!(FieldPath::new("id.nested").missing_prefix(input), Some(FieldPath::new("id.nested")));
        assert_eq!(FieldPath::new("gone.nested.deeper").missing_prefix(input), Some(FieldPath::new("gone.nested")));
    }

    #[test]
    fn a_path_the_record_has_has_no_missing_prefix_even_when_its_value_is_null() {
        let variables = RunVariables::none();
        let data = json!({"address": {"city": null}});
        let input = TemplateInput::new(&data, &variables);

        assert_eq!(FieldPath::new("address.city").missing_prefix(input), None);
        assert_eq!(FieldPath::new("address").missing_prefix(input), None);
    }

    #[test]
    fn the_last_segment_is_the_name_after_the_last_dot() {
        assert_eq!(FieldPath::new("address.street.name").last_segment(), "name");
        assert_eq!(FieldPath::new("id").last_segment(), "id");
    }

    #[test]
    fn displays_as_its_path() {
        assert_eq!(FieldPath::new("properties.tipo").to_string(), "properties.tipo");
        assert_eq!(FieldPath::new("vars.network").to_string(), "vars.network");
    }

    #[test]
    fn a_record_field_called_vars_displays_through_this() {
        assert_eq!(FieldPath::in_record("vars.network").to_string(), "this.vars.network");
        assert_eq!(FieldPath::in_record("variant").to_string(), "variant");
    }
}
