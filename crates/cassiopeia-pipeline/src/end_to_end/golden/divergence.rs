use jsonptr::{PointerBuf, Token};
use serde_json::Value;

/// One step from a JSON value into a value it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step<'a> {
    /// The member of an object with this name.
    Member(&'a str),
    /// The item of an array at this position.
    Item(usize),
}

// `jsonptr` encodes a token from a name or a position; a step is one of the two.
impl<'a> From<Step<'a>> for Token<'a> {
    fn from(step: Step<'a>) -> Token<'a> {
        match step {
            Step::Member(name) => Token::new(name),
            Step::Item(position) => Token::from(position),
        }
    }
}

/// The first place where a value a run wrote departs from the value its snapshot stores.
///
/// Objects are compared member by member in the order the snapshot lists them, then the members
/// only the run wrote; arrays item by item. The place is the first member or item whose values
/// differ in kind or content, or that only one side holds.
#[derive(Debug, PartialEq)]
pub(crate) struct Divergence<'a> {
    /// The steps from the root to the place, outermost first.
    steps: Vec<Step<'a>>,
    /// What the snapshot stores there, or `None` when it stores nothing there.
    stored: Option<&'a Value>,
    /// What the run wrote there, or `None` when it wrote nothing there.
    written: Option<&'a Value>,
}

impl<'a> Divergence<'a> {
    /// Where `written` first departs from `stored`, or `None` when the two are equal.
    pub(crate) fn between(stored: &'a Value, written: &'a Value) -> Option<Divergence<'a>> {
        let mut divergence = diverge(stored, written)?;
        divergence.steps.reverse();
        Some(divergence)
    }

    /// The steps from the root to the place, outermost first.
    pub(crate) fn steps(&self) -> &[Step<'a>] {
        &self.steps
    }

    /// What the snapshot stores at the place, or `None` when it stores nothing there.
    pub(crate) const fn stored(&self) -> Option<&'a Value> {
        self.stored
    }

    /// What the run wrote at the place, or `None` when it wrote nothing there.
    pub(crate) const fn written(&self) -> Option<&'a Value> {
        self.written
    }

    /// The RFC 6901 JSON Pointer to the place, from the root past the first `skipped` steps.
    pub(crate) fn pointer_after(&self, skipped: usize) -> PointerBuf {
        PointerBuf::from_tokens(self.steps.iter().skip(skipped).copied())
    }

    /// A divergence found right at `stored` and `written`.
    const fn here(stored: Option<&'a Value>, written: Option<&'a Value>) -> Divergence<'a> {
        Divergence {
            steps: Vec::new(),
            stored,
            written,
        }
    }

    /// This divergence, found one `step` further in.
    ///
    /// The steps are gathered innermost first while the comparison unwinds, and turned outermost
    /// first once, by `between`.
    fn within(mut self, step: Step<'a>) -> Divergence<'a> {
        self.steps.push(step);
        self
    }
}

/// Where `written` first departs from `stored`, its steps innermost first.
fn diverge<'a>(stored: &'a Value, written: &'a Value) -> Option<Divergence<'a>> {
    match (stored, written) {
        (Value::Object(stored), Value::Object(written)) => stored
            .iter()
            .find_map(|(name, stored)| {
                match written.get(name) {
                    Some(written) => diverge(stored, written),
                    None => Some(Divergence::here(Some(stored), None)),
                }
                .map(|divergence| divergence.within(Step::Member(name)))
            })
            .or_else(|| {
                written
                    .iter()
                    .find(|(name, _)| !stored.contains_key(*name))
                    .map(|(name, written)| Divergence::here(None, Some(written)).within(Step::Member(name)))
            }),
        (Value::Array(stored), Value::Array(written)) => (0..stored.len().max(written.len())).find_map(|position| {
            match (stored.get(position), written.get(position)) {
                (Some(stored), Some(written)) => diverge(stored, written),
                (stored, written) => Some(Divergence::here(stored, written)),
            }
            .map(|divergence| divergence.within(Step::Item(position)))
        }),
        (stored, written) => (stored != written).then_some(Divergence::here(Some(stored), Some(written))),
    }
}

#[cfg(test)]
mod tests {
    use crate::end_to_end::golden::divergence::{Divergence, Step};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn equal_values_have_no_divergence() {
        let value = json!({ "id": "urn:a", "temperature": { "type": "Property", "value": [1, 2] } });

        assert_eq!(Divergence::between(&value, &value), None);
    }

    #[test]
    fn a_nested_value_that_differs_is_found_at_its_member() {
        let stored = json!({ "id": "urn:a", "temperature": { "type": "Property", "value": 21.5 } });
        let written = json!({ "id": "urn:a", "temperature": { "type": "Property", "value": 22 } });

        let divergence = Divergence::between(&stored, &written).unwrap();

        assert_eq!(divergence.steps(), [Step::Member("temperature"), Step::Member("value")]);
        assert_eq!(divergence.stored(), Some(&json!(21.5)));
        assert_eq!(divergence.written(), Some(&json!(22)));
        assert_eq!(divergence.pointer_after(0).to_string(), "/temperature/value");
    }

    #[test]
    fn a_member_the_run_left_out_is_stored_only() {
        let stored = json!({ "id": "urn:a", "name": "Castle" });
        let written = json!({ "id": "urn:a" });

        let divergence = Divergence::between(&stored, &written).unwrap();

        assert_eq!(divergence.steps(), [Step::Member("name")]);
        assert_eq!(divergence.stored(), Some(&json!("Castle")));
        assert_eq!(divergence.written(), None);
    }

    #[test]
    fn a_member_only_the_run_wrote_is_found_after_every_stored_member() {
        let stored = json!({ "id": "urn:a", "name": "Castle" });
        let written = json!({ "id": "urn:a", "height": 3, "name": "Tower" });

        let divergence = Divergence::between(&stored, &written).unwrap();

        assert_eq!(divergence.steps(), [Step::Member("name")]);
        assert_eq!(divergence.stored(), Some(&json!("Castle")));
        assert_eq!(divergence.written(), Some(&json!("Tower")));

        let written = json!({ "id": "urn:a", "height": 3, "name": "Castle" });
        let divergence = Divergence::between(&stored, &written).unwrap();

        assert_eq!(divergence.steps(), [Step::Member("height")]);
        assert_eq!(divergence.stored(), None);
        assert_eq!(divergence.written(), Some(&json!(3)));
    }

    #[test]
    fn an_array_item_that_differs_is_found_at_its_position() {
        let stored = json!([{ "id": "urn:a" }, { "id": "urn:b", "codes": ["BS", "IN"] }]);
        let written = json!([{ "id": "urn:a" }, { "id": "urn:b", "codes": ["BS", "LJ"] }]);

        let divergence = Divergence::between(&stored, &written).unwrap();

        assert_eq!(divergence.steps(), [Step::Item(1), Step::Member("codes"), Step::Item(1)]);
        assert_eq!(divergence.pointer_after(1).to_string(), "/codes/1");
    }

    #[test]
    fn an_item_past_the_end_of_one_side_is_held_by_the_other_only() {
        let stored = json!([{ "id": "urn:a" }]);
        let written = json!([{ "id": "urn:a" }, { "id": "urn:b" }]);

        let divergence = Divergence::between(&stored, &written).unwrap();

        assert_eq!(divergence.steps(), [Step::Item(1)]);
        assert_eq!(divergence.stored(), None);
        assert_eq!(divergence.written(), Some(&json!({ "id": "urn:b" })));

        let divergence = Divergence::between(&written, &stored).unwrap();

        assert_eq!(divergence.steps(), [Step::Item(1)]);
        assert_eq!(divergence.stored(), Some(&json!({ "id": "urn:b" })));
        assert_eq!(divergence.written(), None);
    }

    #[test]
    fn values_of_different_kinds_diverge_where_they_sit() {
        let stored = json!({ "address": { "streetAddress": "Ring" } });
        let written = json!({ "address": "Ring" });

        let divergence = Divergence::between(&stored, &written).unwrap();

        assert_eq!(divergence.steps(), [Step::Member("address")]);
        assert_eq!(divergence.stored(), Some(&json!({ "streetAddress": "Ring" })));
        assert_eq!(divergence.written(), Some(&json!("Ring")));
    }

    #[test]
    fn the_pointer_escapes_a_member_name_holding_a_slash_or_a_tilde() {
        let stored = json!({ "a/b": { "c~d": 1 } });
        let written = json!({ "a/b": { "c~d": 2 } });

        let divergence = Divergence::between(&stored, &written).unwrap();

        assert_eq!(divergence.pointer_after(0).to_string(), "/a~1b/c~0d");
    }
}
