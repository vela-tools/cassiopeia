use crate::{instance_index::InstanceIndex, relationship_key_error::RelationshipKeyError, relationship_path::RelationshipPath};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// The separator between an attribute name and an instance index in a [`RelationshipKey`]'s text
/// form.
///
/// A [`NameBuf`]'s grammar (ETSI GS CIM 009 v1.9.1 clause 4.6.2) forbids `#`, and so does a
/// [`RelationshipPath`] built from names, so the text form of an instance key can never be mistaken
/// for a path and decodes back into the very same name and index.
const INSTANCE_SEPARATOR: char = '#';

/// Where a minted relationship object is recorded on the entity that carries the relationship.
///
/// A plain relationship collects every object it mints under its [`RelationshipPath`], one object
/// list per path. A multi-attribute relationship (ETSI GS CIM 009 v1.9.1 clause 4.5.5) is several
/// `datasetId`-tagged instances under one attribute name, so each object is recorded under the
/// instance that minted it instead: the instance boundaries travel with the objects from the expander
/// to the transformer, and no later stage re-derives them from the source record. Instances are only
/// declared on top-level attributes, so an instance key names the attribute itself rather than a
/// path.
///
/// The text form is the store key a relationship store groups objects by: a path in its dotted form
/// (`hasLeadActor.playsCharacter`), or an attribute name and an instance index joined by `#`
/// (`servesAirports#1`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, Serialize, Deserialize)]
pub enum RelationshipKey {
    /// Every object of the relationship reached by this path.
    #[display("{_0}")]
    Path(RelationshipPath),
    /// The objects one instance of a multi-attribute relationship minted.
    #[display("{attribute}{INSTANCE_SEPARATOR}{index}")]
    Instance {
        /// The top-level attribute declaring the instances.
        attribute: NameBuf,
        /// The instance's position in the attribute's declared `instances`.
        index: InstanceIndex,
    },
}

// `derive_more::FromStr` only derives parsing for newtypes and field-less enums; reading a key back
// means choosing the variant from the text itself, so it is written by hand.
impl FromStr for RelationshipKey {
    type Err = RelationshipKeyError;

    fn from_str(value: &str) -> Result<RelationshipKey, RelationshipKeyError> {
        match value.split_once(INSTANCE_SEPARATOR) {
            Some((attribute, index)) => Ok(RelationshipKey::Instance {
                attribute: NameBuf::new(attribute)?,
                index: index.parse().map_err(|source| RelationshipKeyError::InstanceIndex {
                    rejected: index.into(),
                    source,
                })?,
            }),
            None => Ok(RelationshipKey::Path(value.parse()?)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        instance_index::InstanceIndex,
        relationship_key::RelationshipKey,
        relationship_key_error::RelationshipKeyError,
        relationship_path::RelationshipPath,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn a_flat_path_key_reads_as_its_attribute_name_and_round_trips() {
        let key = RelationshipKey::Path(RelationshipPath::flat(name("refRoad")));

        assert_eq!(key.to_string(), "refRoad");
        assert_eq!("refRoad".parse::<RelationshipKey>().unwrap(), key);
    }

    #[test]
    fn a_nested_path_key_reads_as_its_dotted_path_and_round_trips() {
        let key = RelationshipKey::Path(RelationshipPath::flat(name("hasLeadActor")).push(name("playsCharacter")));

        assert_eq!(key.to_string(), "hasLeadActor.playsCharacter");
        assert_eq!("hasLeadActor.playsCharacter".parse::<RelationshipKey>().unwrap(), key);
    }

    #[test]
    fn an_instance_key_joins_its_attribute_and_index_and_round_trips() {
        let key = RelationshipKey::Instance {
            attribute: name("servesAirports"),
            index: InstanceIndex::from(1),
        };

        assert_eq!(key.to_string(), "servesAirports#1");
        assert_eq!("servesAirports#1".parse::<RelationshipKey>().unwrap(), key);
    }

    #[test]
    fn a_prefixed_attribute_name_survives_an_instance_key() {
        let key = RelationshipKey::Instance {
            attribute: name("schema:serves"),
            index: InstanceIndex::from(0),
        };

        assert_eq!(key.to_string().parse::<RelationshipKey>().unwrap(), key);
    }

    #[test]
    fn an_instance_key_whose_index_is_not_a_count_is_rejected() {
        let error = "servesAirports#first".parse::<RelationshipKey>().unwrap_err();

        assert!(matches!(error, RelationshipKeyError::InstanceIndex { rejected, .. } if &*rejected == "first"));
    }

    #[test]
    fn an_instance_key_whose_attribute_is_not_a_name_is_rejected() {
        let error = "serves.Airports#0".parse::<RelationshipKey>().unwrap_err();

        assert!(matches!(error, RelationshipKeyError::Name(_)));
    }

    #[test]
    fn a_path_key_with_an_illegal_segment_is_rejected() {
        let error = "hasLeadActor.9bad".parse::<RelationshipKey>().unwrap_err();

        assert!(matches!(error, RelationshipKeyError::Name(_)));
    }
}
