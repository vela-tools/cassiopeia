use crate::location_record::{level::Level, parameter_name::ParameterName};
use derive_more::IntoIterator;
use serde_json::{Map, Value};

/// The record key a level's type token is emitted under.
pub const LEVEL_TYPE: &str = "level_type";

/// The record key a level's numeric value is emitted under.
pub const LEVEL: &str = "level";

/// The fields every location record of one grid slice, plane, or observation shares: its level, its
/// times, and any other coordinate the source attaches to all of its cells.
///
/// They are resolved once per slice and stamped onto each cell's record by
/// [`crate::location_record::record::location_record`], in insertion order, so every format emitting
/// location records keys and orders them the same way.
#[derive(Debug, Clone, Default, IntoIterator)]
#[into_iterator(ref)]
pub struct LocationFacets(Map<String, Value>);

impl LocationFacets {
    /// Adds a normalized level: `level_type` is the bare token and `level` its numeric value in the
    /// type's canonical unit, absent for the surface-like types that carry no scalar.
    #[must_use]
    pub fn with_level(mut self, level: &Level) -> LocationFacets {
        self.0.insert(LEVEL_TYPE.to_owned(), Value::String(level.type_token().to_owned()));
        match level.value().and_then(whole_preserving_number) {
            Some(value) => self.with_entry(LEVEL, value),
            None => self,
        }
    }

    /// Adds an RFC 3339 timestamp under `key`.
    #[must_use]
    pub fn with_time(self, key: &ParameterName, time: String) -> LocationFacets {
        self.with_value(key, Value::String(time))
    }

    /// Adds a number under `key`, a whole value as a JSON integer; a non-finite value adds nothing.
    #[must_use]
    pub fn with_number(self, key: &ParameterName, value: f64) -> LocationFacets {
        match whole_preserving_number(value) {
            Some(value) => self.with_value(key, value),
            None => self,
        }
    }

    /// Adds an arbitrary JSON value under `key`.
    #[must_use]
    pub fn with_value(self, key: &ParameterName, value: Value) -> LocationFacets {
        self.with_entry(key.as_str(), value)
    }

    fn with_entry(mut self, key: &str, value: Value) -> LocationFacets {
        self.0.insert(key.to_owned(), value);
        self
    }
}

/// Renders a finite float as a JSON number, preserving whole numbers as integers.
///
/// A whole level or coordinate (2 m, 850 hPa, ensemble member 3) reads better as `2` than as `2.0`,
/// and Rust's float `Display` drops a trailing zero (`2.0` -> "2"), so re-parsing that text lands a
/// whole value as a JSON integer and a fractional one (sigma 0.995) as a float, without a lossy
/// `f64`-to-integer cast. A non-finite value has no JSON representation and yields `None`.
fn whole_preserving_number(value: f64) -> Option<Value> {
    if !value.is_finite() {
        return None;
    }
    serde_json::from_str::<Value>(&format!("{value}")).ok()
}

#[cfg(test)]
mod tests {
    use crate::location_record::{
        facets::LocationFacets,
        level::{Level, LevelType},
        parameter_name::ParameterName,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn entries(facets: &LocationFacets) -> Vec<(String, Value)> {
        facets.into_iter().map(|(key, value)| (key.clone(), value.clone())).collect()
    }

    #[test]
    fn a_level_adds_its_token_and_a_whole_value_as_an_integer() {
        let facets = LocationFacets::default().with_level(&Level::new(LevelType::Isobaric, Some(850.0)));
        assert_eq!(
            entries(&facets),
            vec![("level_type".to_string(), json!("isobaric")), ("level".to_string(), json!(850))]
        );
    }

    #[test]
    fn a_level_without_a_value_adds_only_its_token() {
        let facets = LocationFacets::default().with_level(&Level::new(LevelType::Surface, None));
        assert_eq!(entries(&facets), vec![("level_type".to_string(), json!("surface"))]);
    }

    #[test]
    fn a_fractional_level_stays_a_float() {
        let facets = LocationFacets::default().with_level(&Level::new(LevelType::Sigma, Some(0.995)));
        assert_eq!(entries(&facets).pop().map(|(_, value)| value), Some(json!(0.995)));
    }

    #[test]
    fn times_numbers_and_values_keep_their_insertion_order() {
        let facets = LocationFacets::default()
            .with_time(&ParameterName::from("time"), "2026-01-01T00:00:00+00:00".to_string())
            .with_number(&ParameterName::from("number"), 3.0)
            .with_value(&ParameterName::from("station_id"), json!("A1"));
        assert_eq!(
            entries(&facets),
            vec![
                ("time".to_string(), json!("2026-01-01T00:00:00+00:00")),
                ("number".to_string(), json!(3)),
                ("station_id".to_string(), json!("A1")),
            ]
        );
    }

    #[test]
    fn a_non_finite_number_adds_nothing() {
        let facets = LocationFacets::default().with_number(&ParameterName::from("number"), f64::NAN);
        assert_eq!(entries(&facets), Vec::new());
    }
}
