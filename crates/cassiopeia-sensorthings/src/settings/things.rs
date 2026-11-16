use crate::settings::{
    entity_filter::{EntityFilter, EntityFilterError},
    filter_setting::FilterSetting,
};
use serde::{Deserialize, Serialize};

/// Which Things a latest read lists by themselves, besides the Things of its data streams: the
/// `things` member of a `sensorthings` block.
///
/// The member is `false` for none, `true` for every one, or an `OData` `$filter` expression over
/// Things for the ones it matches. A Thing may have no data stream (OGC 18-088 section 8.2.1, Table
/// 4), such as a station whose sensors are not connected yet, and a read that finds Things through
/// their data streams never finds it. Listing Things takes a traversal of the Things collection of
/// its own, so a read lists them only when told to. A range or incremental read lists none: it gives
/// one record per Observation, and a Thing without data streams has no Observation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(try_from = "ThingsMember", into = "ThingsMember")]
pub enum Things {
    /// No Thing is listed beyond the Things of the data streams read (`things: false`, or no
    /// member).
    #[default]
    OfDatastreams,
    /// Every Thing is listed (`things: true`).
    Every,
    /// The Things the filter matches are listed.
    Filtered(EntityFilter),
}

/// The `things` member as it is written: a boolean, or an expression.
#[derive(Serialize, Deserialize)]
#[serde(untagged, expecting = "`things` is `true`, `false` or an OData expression over Things")]
enum ThingsMember {
    /// `true` for every Thing, `false` for none beyond the Things of the data streams read.
    Selected(bool),
    /// The expression that selects Things.
    Expression(String),
}

impl TryFrom<ThingsMember> for Things {
    type Error = EntityFilterError;

    fn try_from(member: ThingsMember) -> Result<Things, EntityFilterError> {
        match member {
            ThingsMember::Selected(true) => Ok(Things::Every),
            ThingsMember::Selected(false) => Ok(Things::OfDatastreams),
            ThingsMember::Expression(text) => EntityFilter::new(FilterSetting::Things, text).map(Things::Filtered),
        }
    }
}

// Written by hand: no derive maps the variants of a selection onto the boolean or the text they are
// written as.
impl From<Things> for ThingsMember {
    fn from(selection: Things) -> ThingsMember {
        match selection {
            Things::OfDatastreams => ThingsMember::Selected(false),
            Things::Every => ThingsMember::Selected(true),
            Things::Filtered(filter) => ThingsMember::Expression(Box::<str>::from(filter).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::{entity_filter::EntityFilter, filter_setting::FilterSetting, things::Things};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn a_boolean_lists_every_thing_or_none_beyond_those_of_the_data_streams() {
        assert_eq!(serde_json::from_value::<Things>(json!(true)).unwrap(), Things::Every);
        assert_eq!(serde_json::from_value::<Things>(json!(false)).unwrap(), Things::OfDatastreams);
        assert_eq!(Things::default(), Things::OfDatastreams);
    }

    #[test]
    fn an_expression_lists_the_things_it_matches() {
        assert_eq!(
            serde_json::from_value::<Things>(json!("startswith(name,'ILM_')")).unwrap(),
            Things::Filtered(EntityFilter::new(FilterSetting::Things, "startswith(name,'ILM_')".to_owned()).unwrap())
        );
    }

    #[test]
    fn the_selection_is_written_back_as_it_is_read() {
        for value in [json!(true), json!(false), json!("startswith(name,'ILM_')")] {
            let selection: Things = serde_json::from_value(value.clone()).unwrap();

            assert_eq!(serde_json::to_value(selection).unwrap(), value);
        }
    }

    #[test]
    fn a_blank_expression_is_refused_naming_the_setting() {
        let error = serde_json::from_value::<Things>(json!("  ")).unwrap_err();

        assert!(error.to_string().contains("a `things` filter is an OData expression"), "{error}");
    }

    #[test]
    fn anything_but_a_boolean_or_an_expression_is_refused() {
        for value in [json!(1), json!(null), json!(["a"]), json!({"filter": "a"})] {
            let error = serde_json::from_value::<Things>(value.clone()).unwrap_err();

            assert!(
                error.to_string().contains("`things` is `true`, `false` or an OData expression over Things"),
                "{value}: {error}"
            );
        }
    }
}
