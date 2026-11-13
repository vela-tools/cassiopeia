use cassiopeia_unit_notation::resolution::resolve;
use cefact_units::UnitCode;

/// The `unitOfMeasurement` of a Datastream (OGC 18-088 section 8.2.4, Table 10), as far as its text
/// members describe the unit.
///
/// Each of `name`, `symbol` and `definition` describes the unit, and a server may fill in any of
/// them, or none, as it does for a Datastream whose Observations have no unit. Every component that
/// asks what a unit is reads it here, so the `cefact_unit` filter and the readings a server offers
/// always give one unit the same code and the same text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnitOfMeasurement<'a> {
    /// The unit's `name`, such as `microgram per cubic metre`.
    name: Option<&'a str>,
    /// The unit's `symbol`, such as `ug/m3`.
    symbol: Option<&'a str>,
    /// The unit's `definition`, such as `https://qudt.org/vocab/unit/MicroGM-PER-M3`.
    definition: Option<&'a str>,
}

impl<'a> UnitOfMeasurement<'a> {
    /// The unit whose `name`, `symbol` and `definition` `member` reads by their member names, each
    /// `None` when the unit has no text there.
    pub fn read(member: impl Fn(&'static str) -> Option<&'a str>) -> UnitOfMeasurement<'a> {
        UnitOfMeasurement {
            name: member("name"),
            symbol: member("symbol"),
            definition: member("definition"),
        }
    }

    /// The UN/CEFACT Recommendation No. 20 code the unit denotes: the code of its `definition`,
    /// which identifies the unit, else of its `symbol` or its `name`, which only spell it; `None`
    /// when none of them resolves.
    #[must_use]
    pub fn code(&self) -> Option<UnitCode> {
        [self.definition, self.symbol, self.name].into_iter().flatten().find_map(resolve)
    }

    /// The text a reader recognises the unit by soonest: its `symbol`, else its `name`, else its
    /// `definition`; `None` when it has none of them.
    #[must_use]
    pub fn text(&self) -> Option<&'a str> {
        self.symbol.or(self.name).or(self.definition)
    }
}

#[cfg(test)]
mod tests {
    use crate::unit_of_measurement::UnitOfMeasurement;
    use cefact_units::UnitCode;
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    fn unit(value: &Value) -> UnitOfMeasurement<'_> {
        let members: &Map<String, Value> = value.as_object().unwrap();
        UnitOfMeasurement::read(|member| members.get(member).and_then(Value::as_str))
    }

    #[test]
    fn the_definition_resolves_before_the_symbol_and_the_name() {
        let served = json!({"definition": "https://qudt.org/vocab/unit/DEG_C", "symbol": "K", "name": "kelvin"});

        assert_eq!(unit(&served).code(), Some(UnitCode::Cel));
    }

    #[test]
    fn a_member_that_does_not_resolve_gives_way_to_the_next() {
        let served = json!({"definition": "http://unitsofmeasure.org/ucum.html#para-30", "symbol": "xyzzy", "name": "kelvin"});

        assert_eq!(unit(&served).code(), Some(UnitCode::Kel));
    }

    #[test]
    fn the_symbol_is_the_text_before_the_name_and_the_definition() {
        assert_eq!(unit(&json!({"definition": "urn:d", "symbol": "s", "name": "n"})).text(), Some("s"));
        assert_eq!(unit(&json!({"definition": "urn:d", "name": "n"})).text(), Some("n"));
        assert_eq!(unit(&json!({"definition": "urn:d"})).text(), Some("urn:d"));
    }

    #[test]
    fn a_unit_without_text_has_no_code_and_no_text() {
        let served = json!({"name": null, "symbol": null, "definition": null});

        assert_eq!(unit(&served), UnitOfMeasurement::default());
        assert_eq!(unit(&served).code(), None);
        assert_eq!(unit(&served).text(), None);
    }

    #[test]
    fn an_unresolved_unit_keeps_its_text() {
        let served = json!({"symbol": "xyzzy"});

        assert_eq!(unit(&served).code(), None);
        assert_eq!(unit(&served).text(), Some("xyzzy"));
    }
}
