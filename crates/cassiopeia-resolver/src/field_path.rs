use derive_more::Display;
use serde::{Deserialize, Serialize};

/// Where in a source record a field sits: the object keys leading to it from the record's root.
///
/// The keys are the source format's own field names, so they are carried as written. The derived
/// order compares key by key and puts a path before every path it is a prefix of, which is what lets
/// a merge name the same field whichever order its records arrived in (see
/// [`deep_merge`](crate::entity_store::merge::deep_merge)).
#[derive(Debug, Clone, Display, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[display("{}", _0.join("."))]
pub struct FieldPath(Vec<Box<str>>);

impl FieldPath {
    /// Builds a path from the keys leading to the field, outermost first.
    #[must_use]
    pub fn new(segments: &[&str]) -> FieldPath {
        FieldPath(segments.iter().map(|segment| Box::from(*segment)).collect())
    }

    /// The earlier of two optional paths, either standing alone when the other is absent.
    #[must_use]
    pub fn earliest(left: Option<FieldPath>, right: Option<FieldPath>) -> Option<FieldPath> {
        match (left, right) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (found @ Some(_), None) | (None, found @ Some(_)) => found,
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::field_path::FieldPath;

    #[test]
    fn a_path_renders_its_keys_joined_by_dots() {
        assert_eq!(
            FieldPath::new(&["placemark", "geometry", "coordinates"]).to_string(),
            "placemark.geometry.coordinates"
        );
    }

    #[test]
    fn a_path_orders_before_every_path_it_is_a_prefix_of_and_otherwise_key_by_key() {
        assert!(FieldPath::new(&["a"]) < FieldPath::new(&["a", "b"]));
        assert!(FieldPath::new(&["a", "z"]) < FieldPath::new(&["b"]));
    }

    #[test]
    fn the_earliest_of_two_paths_is_the_lower_and_an_absent_path_never_wins() {
        let a = FieldPath::new(&["a"]);
        let b = FieldPath::new(&["b"]);

        assert_eq!(FieldPath::earliest(Some(b.clone()), Some(a.clone())), Some(a));
        assert_eq!(FieldPath::earliest(None, Some(b.clone())), Some(b.clone()));
        assert_eq!(FieldPath::earliest(Some(b.clone()), None), Some(b));
        assert_eq!(FieldPath::earliest(None, None), None);
    }
}
