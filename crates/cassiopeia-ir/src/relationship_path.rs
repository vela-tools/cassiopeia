use cassiopeia_ngsi_ld::entity::{error::NgsiLdError, name::NameBuf};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Display, Formatter, Result as FmtResult, Write},
    slice::from_ref,
    str::FromStr,
};

/// The separator between path segments in a [`RelationshipPath`]'s dotted string form.
///
/// A [`NameBuf`]'s grammar (ETSI GS CIM 009 v1.9.1 clause 4.6.2) forbids `.`, so a path encoded by
/// joining its segments with `.` decodes back into the very same segments without ambiguity.
const SEGMENT_SEPARATOR: char = '.';

/// The path from an entity to one of its relationship objects, named by the attribute names it
/// descends through.
///
/// A top-level relationship is reached by a single attribute name: a one-segment path such as
/// `directedBy`. A relationship declared as a sub-attribute of another attribute is reached by
/// descending into that attribute's `properties`, so it carries a multi-segment path such as
/// `directedBy.playsCharacter` (a relationship carried by a relationship, ETSI GS CIM 009 v1.9.1
/// clause 4.5.2.2 with 4.5.3). The single-segment [`RelationshipPath::Flat`] case is stored as a bare
/// [`NameBuf`], so the overwhelmingly common top-level relationship carries no extra allocation over
/// the name itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationshipPath {
    /// A top-level relationship, reached by one attribute name.
    Flat(NameBuf),
    /// A nested relationship, reached by descending through two or more attribute names.
    Nested(Box<[NameBuf]>),
}

impl RelationshipPath {
    /// Builds the one-segment path of a top-level relationship.
    #[must_use]
    pub const fn flat(name: NameBuf) -> RelationshipPath {
        RelationshipPath::Flat(name)
    }

    /// Builds a path from its ordered segments, collapsing a single segment to the [`Flat`](Self::Flat)
    /// form.
    ///
    /// An empty segment list is an invalid path; it is represented as an empty [`Nested`](Self::Nested)
    /// rather than rejected, since every construction site supplies at least one segment.
    #[must_use]
    pub fn from_segments(segments: Vec<NameBuf>) -> RelationshipPath {
        if segments.len() == 1 {
            // A single segment is a flat path; move it out rather than allocating a boxed slice.
            let mut segments = segments;
            RelationshipPath::Flat(segments.remove(0))
        } else {
            RelationshipPath::Nested(segments.into_boxed_slice())
        }
    }

    /// The ordered attribute-name segments this path descends through.
    #[must_use]
    pub fn segments(&self) -> &[NameBuf] {
        match self {
            RelationshipPath::Flat(name) => from_ref(name),
            RelationshipPath::Nested(segments) => segments,
        }
    }

    /// Whether this is a one-segment path, that is, a top-level relationship.
    #[must_use]
    pub const fn is_flat(&self) -> bool {
        matches!(self, RelationshipPath::Flat(_))
    }

    /// Extends this path by one deeper attribute name, yielding the path of a sub-attribute.
    #[must_use]
    pub fn push(&self, name: NameBuf) -> RelationshipPath {
        let mut segments: Vec<NameBuf> = self.segments().to_vec();
        segments.push(name);
        RelationshipPath::from_segments(segments)
    }
}

impl Display for RelationshipPath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        let mut segments = self.segments().iter();
        if let Some(first) = segments.next() {
            formatter.write_str(first.as_str())?;
            for segment in segments {
                formatter.write_char(SEGMENT_SEPARATOR)?;
                formatter.write_str(segment.as_str())?;
            }
        }
        Ok(())
    }
}

impl FromStr for RelationshipPath {
    type Err = NgsiLdError;

    fn from_str(value: &str) -> Result<RelationshipPath, NgsiLdError> {
        let segments = value
            .split(SEGMENT_SEPARATOR)
            .map(NameBuf::new)
            .collect::<Result<Vec<NameBuf>, NgsiLdError>>()?;
        Ok(RelationshipPath::from_segments(segments))
    }
}

#[cfg(test)]
mod tests {
    use crate::relationship_path::RelationshipPath;
    use cassiopeia_ngsi_ld::entity::name::NameBuf;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn a_single_segment_path_is_flat() {
        let path = RelationshipPath::flat(name("directedBy"));

        assert!(path.is_flat());
        assert_eq!(path.segments(), &[name("directedBy")]);
        assert_eq!(path.to_string(), "directedBy");
    }

    #[test]
    fn from_segments_collapses_one_segment_to_flat() {
        let path = RelationshipPath::from_segments(vec![name("directedBy")]);

        assert!(path.is_flat());
    }

    #[test]
    fn a_multi_segment_path_is_nested_and_joins_with_dots() {
        let path = RelationshipPath::from_segments(vec![name("directedBy"), name("playsCharacter")]);

        assert!(!path.is_flat());
        assert_eq!(path.segments(), &[name("directedBy"), name("playsCharacter")]);
        assert_eq!(path.to_string(), "directedBy.playsCharacter");
    }

    #[test]
    fn push_extends_a_flat_path_into_a_nested_one() {
        let path = RelationshipPath::flat(name("directedBy")).push(name("playsCharacter"));

        assert_eq!(path.to_string(), "directedBy.playsCharacter");
    }

    #[test]
    fn a_flat_path_round_trips_through_its_string_form() {
        let path: RelationshipPath = "directedBy".parse().expect("parses");

        assert_eq!(path, RelationshipPath::flat(name("directedBy")));
    }

    #[test]
    fn a_nested_path_round_trips_through_its_string_form() {
        let path: RelationshipPath = "hasLeadActor.playsCharacter.locatedIn".parse().expect("parses");

        assert_eq!(
            path,
            RelationshipPath::from_segments(vec![name("hasLeadActor"), name("playsCharacter"), name("locatedIn")])
        );
        assert_eq!(path.to_string(), "hasLeadActor.playsCharacter.locatedIn");
    }

    #[test]
    fn a_segment_that_is_not_a_valid_name_is_rejected() {
        assert!("hasLeadActor.9bad".parse::<RelationshipPath>().is_err());
    }
}
