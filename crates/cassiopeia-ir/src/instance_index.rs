use derive_more::{Display, From, FromStr, Into};
use serde::{Deserialize, Serialize};

/// The position of one instance in an attribute's declared `instances` list, counted from zero.
///
/// A multi-attribute (ETSI GS CIM 009 v1.9.1 clause 4.5.5) is declared as an ordered list of
/// `datasetId`-tagged instances. The expander tags every object it mints for an instance with that
/// instance's index, and the extractor records each instance's metadata at the same index, so the
/// transformer pairs an instance's objects with its own `datasetId` by index rather than by counting
/// surviving instances. An instance that mints no object simply has no objects under its index; it
/// cannot shift any other instance onto the wrong metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display, FromStr, From, Into, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceIndex(usize);

#[cfg(test)]
mod tests {
    use crate::instance_index::InstanceIndex;

    #[test]
    fn an_index_round_trips_through_its_text_form() {
        let index = InstanceIndex::from(3);

        assert_eq!(index.to_string(), "3");
        assert_eq!("3".parse::<InstanceIndex>().unwrap(), index);
        assert_eq!(usize::from(index), 3);
    }

    #[test]
    fn text_that_is_not_a_count_is_rejected() {
        assert!("-1".parse::<InstanceIndex>().is_err());
        assert!("first".parse::<InstanceIndex>().is_err());
    }

    #[test]
    fn indices_order_by_declaration_position() {
        assert!(InstanceIndex::from(0) < InstanceIndex::from(1));
    }
}
