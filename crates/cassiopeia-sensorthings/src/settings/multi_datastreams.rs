use crate::{
    settings::stream_filter::{StreamFilter, StreamFilterError},
    stream_kind::StreamKind,
};
use serde::{Deserialize, Serialize};

/// Which `MultiDatastreams` a read reads: the `multiDatastreams` member of a `sensorthings` block.
///
/// The member is `false` for none, `true` for every one, or an `OData` `$filter` expression over
/// `MultiDatastreams` for the ones it matches. A read without the member reads none, so a manifest
/// that selects its Datastreams sends no request to the `MultiDatastreams` collection, which a server
/// without the `MultiDatastream` extension (OGC 18-088 section 12) answers with an error.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(try_from = "MultiDatastreamsMember", into = "MultiDatastreamsMember")]
pub enum MultiDatastreams {
    /// No `MultiDatastream` is read (`multiDatastreams: false`, or no member).
    #[default]
    None,
    /// Every `MultiDatastream` is read (`multiDatastreams: true`).
    Every,
    /// The `MultiDatastreams` the filter matches are read.
    Filtered(StreamFilter),
}

/// The `multiDatastreams` member as it is written: a boolean, or an expression.
#[derive(Serialize, Deserialize)]
#[serde(untagged, expecting = "`multiDatastreams` is `true`, `false` or an OData expression over MultiDatastreams")]
enum MultiDatastreamsMember {
    /// `true` for every `MultiDatastream`, `false` for none.
    Selected(bool),
    /// The expression that selects `MultiDatastreams`.
    Expression(String),
}

impl TryFrom<MultiDatastreamsMember> for MultiDatastreams {
    type Error = StreamFilterError;

    fn try_from(member: MultiDatastreamsMember) -> Result<MultiDatastreams, StreamFilterError> {
        match member {
            MultiDatastreamsMember::Selected(true) => Ok(MultiDatastreams::Every),
            MultiDatastreamsMember::Selected(false) => Ok(MultiDatastreams::None),
            MultiDatastreamsMember::Expression(text) => StreamFilter::new(StreamKind::MultiDatastream, text).map(MultiDatastreams::Filtered),
        }
    }
}

// Written by hand: no derive maps the variants of a selection onto the boolean or the text they are
// written as.
impl From<MultiDatastreams> for MultiDatastreamsMember {
    fn from(selection: MultiDatastreams) -> MultiDatastreamsMember {
        match selection {
            MultiDatastreams::None => MultiDatastreamsMember::Selected(false),
            MultiDatastreams::Every => MultiDatastreamsMember::Selected(true),
            MultiDatastreams::Filtered(filter) => MultiDatastreamsMember::Expression(Box::<str>::from(filter).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        settings::{multi_datastreams::MultiDatastreams, stream_filter::StreamFilter},
        stream_kind::StreamKind,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn filtered(text: &str) -> MultiDatastreams {
        MultiDatastreams::Filtered(StreamFilter::new(StreamKind::MultiDatastream, text.to_owned()).unwrap())
    }

    #[test]
    fn a_boolean_selects_every_multi_datastream_or_none() {
        assert_eq!(serde_json::from_value::<MultiDatastreams>(json!(true)).unwrap(), MultiDatastreams::Every);
        assert_eq!(serde_json::from_value::<MultiDatastreams>(json!(false)).unwrap(), MultiDatastreams::None);
        assert_eq!(MultiDatastreams::default(), MultiDatastreams::None);
    }

    #[test]
    fn an_expression_selects_the_multi_datastreams_it_matches() {
        assert_eq!(
            serde_json::from_value::<MultiDatastreams>(json!("startswith(name,'ws-')")).unwrap(),
            filtered("startswith(name,'ws-')")
        );
    }

    #[test]
    fn the_selection_is_written_back_as_it_is_read() {
        for value in [json!(true), json!(false), json!("startswith(name,'ws-')")] {
            let selection: MultiDatastreams = serde_json::from_value(value.clone()).unwrap();

            assert_eq!(serde_json::to_value(selection).unwrap(), value);
        }
    }

    #[test]
    fn a_blank_expression_is_refused_naming_the_setting() {
        let error = serde_json::from_value::<MultiDatastreams>(json!(" ")).unwrap_err();

        assert!(error.to_string().contains("a `multiDatastreams` filter is an OData expression"), "{error}");
    }

    #[test]
    fn anything_but_a_boolean_or_an_expression_is_refused() {
        for value in [json!(1), json!(null), json!(["a"]), json!({"filter": "a"})] {
            let error = serde_json::from_value::<MultiDatastreams>(value.clone()).unwrap_err();

            assert!(
                error.to_string().contains("`multiDatastreams` is `true`, `false` or an OData expression"),
                "{value}: {error}"
            );
        }
    }
}
