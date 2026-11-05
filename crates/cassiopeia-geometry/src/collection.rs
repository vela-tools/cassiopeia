use crate::{convert::from_geojson, error::GeometryError, geometry::NgsiLdGeometry, policy::GeometryPolicy, target::GeometryTarget};
use derive_more::Into;
use geojson::{Geometry, GeometryValue};

/// A `GeometryCollection` (RFC 7946 clause 3.1.8) whose every member was read as a geometry of its
/// own.
///
/// No `GeoProperty` holds one, since it admits only six geometry types (ETSI GS CIM 009 v1.9.1
/// clause 4.7.1), but a Property's value may be any JSON object (clause 4.5.2), so a collection kept
/// as the source spells it is carried as its members, each validated and normalised as any geometry
/// is.
#[derive(Debug, Clone, PartialEq, Into)]
pub struct GeometryCollection(Vec<CollectionMember>);

/// One member of a [`GeometryCollection`].
#[derive(Debug, Clone, PartialEq)]
pub enum CollectionMember {
    /// A member of one of the six geometry types.
    Geometry(NgsiLdGeometry),
    /// A nested collection, which RFC 7946 clause 3.1.8 discourages but admits.
    Collection(GeometryCollection),
}

impl GeometryCollection {
    /// Reads every member of a source `GeometryCollection` as a geometry of its own under `policy`,
    /// keeping a nested collection as a collection of its members.
    ///
    /// # Errors
    /// Returns the [`GeometryError`] of the first member that is no valid geometry.
    pub fn from_geojson(members: Vec<Geometry>, policy: &GeometryPolicy) -> Result<GeometryCollection, GeometryError> {
        members
            .into_iter()
            .map(|member| CollectionMember::from_geojson(member, *policy))
            .collect::<Result<Vec<CollectionMember>, GeometryError>>()
            .map(GeometryCollection)
    }
}

impl CollectionMember {
    /// Reads one member of a collection under `policy`: a geometry as the geometry it spells, its
    /// type preserved, and a nested collection as a collection of its members.
    ///
    /// # Errors
    /// Returns the [`GeometryError`] naming why the member, or a member nested in it, is no valid
    /// geometry.
    fn from_geojson(member: Geometry, policy: GeometryPolicy) -> Result<CollectionMember, GeometryError> {
        match member.value {
            GeometryValue::GeometryCollection { geometries } => GeometryCollection::from_geojson(geometries, &policy).map(CollectionMember::Collection),
            value @ (GeometryValue::Point { .. }
            | GeometryValue::MultiPoint { .. }
            | GeometryValue::LineString { .. }
            | GeometryValue::MultiLineString { .. }
            | GeometryValue::Polygon { .. }
            | GeometryValue::MultiPolygon { .. }) => from_geojson(Geometry::new(value), GeometryTarget::Preserve, &policy).map(CollectionMember::Geometry),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        collection::{CollectionMember, GeometryCollection},
        error::GeometryError,
        geometry::NgsiLdGeometry,
        policy::GeometryPolicy,
    };
    use geojson::{Geometry, GeometryValue};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    /// The members of the collection read from `members` under `policy`.
    fn read(members: Vec<Geometry>, policy: GeometryPolicy) -> Result<Vec<CollectionMember>, GeometryError> {
        GeometryCollection::from_geojson(members, &policy).map(Vec::from)
    }

    /// A `GeoJSON` point at `coordinates`.
    fn point(coordinates: Vec<f64>) -> Geometry {
        Geometry::new(GeometryValue::Point {
            coordinates: coordinates.into(),
        })
    }

    #[test]
    fn each_member_is_read_as_the_geometry_it_spells() {
        assert_eq!(
            read(vec![point(vec![1.0, 2.0])], GeometryPolicy::default()),
            Ok(vec![CollectionMember::Geometry(NgsiLdGeometry::Point {
                coordinates: [1.0, 2.0].into(),
            })])
        );
    }

    #[test]
    fn each_member_is_normalised_under_the_policy() {
        let policy: GeometryPolicy = serde_json::from_value(json!({"altitude": "drop"})).unwrap();

        assert_eq!(
            read(vec![point(vec![1.0, 2.0, 300.0])], policy),
            Ok(vec![CollectionMember::Geometry(NgsiLdGeometry::Point {
                coordinates: [1.0, 2.0].into(),
            })])
        );
    }

    #[test]
    fn a_nested_collection_is_kept_as_a_collection_of_its_members() {
        let nested = Geometry::new(GeometryValue::GeometryCollection {
            geometries: vec![point(vec![3.0, 4.0])],
        });

        assert_eq!(
            read(vec![nested], GeometryPolicy::default()),
            Ok(vec![CollectionMember::Collection(
                GeometryCollection::from_geojson(vec![point(vec![3.0, 4.0])], &GeometryPolicy::default()).unwrap()
            )])
        );
    }

    #[test]
    fn a_member_that_is_no_valid_geometry_refuses_the_collection() {
        let short = Geometry::new(GeometryValue::LineString {
            coordinates: vec![[0.0, 0.0].into()],
        });

        assert_eq!(
            read(vec![point(vec![1.0, 2.0]), short], GeometryPolicy::default()),
            Err(GeometryError::ShortLineString { positions: 1 })
        );
    }
}
