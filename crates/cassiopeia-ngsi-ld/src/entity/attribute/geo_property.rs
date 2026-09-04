use crate::entity::representation::{SerializeRepr, concise, normalized, simplified};
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use cassiopeia_geometry::geometry::NgsiLdGeometry;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, Serializer};
use urn_rs::Urn;

/// An NGSI-LD `GeoProperty` attribute (ETSI GS CIM 009 v1.9.1, clause 4.7).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NgsiLdGeoProperty {
    /// The geometry, restricted to the six types clause 4.7 admits.
    pub value: NgsiLdGeometry,
    /// When the geometry was observed.
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    /// The dataset this instance belongs to (a URI).
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Urn>,
    /// The broker-assigned instance identifier (a URI).
    #[serde(rename = "instanceId", skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<Urn>,
}

impl NgsiLdGeoProperty {
    /// Builds a geo property carrying `geometry` with no qualifiers.
    #[must_use]
    pub const fn new(geometry: NgsiLdGeometry) -> NgsiLdGeoProperty {
        NgsiLdGeoProperty {
            value: geometry,
            observed_at: None,
            dataset_id: None,
            instance_id: None,
        }
    }
}

impl SerializeRepr for NgsiLdGeoProperty {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_geo_property(self, skip_null, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_geo_property(self, skip_null, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_geo_property(self, skip_null, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::attribute::geo_property::NgsiLdGeoProperty;
    use cassiopeia_geometry::geometry::NgsiLdGeometry;

    #[test]
    fn a_new_geo_property_holds_its_geometry_and_no_qualifiers() {
        let geo = NgsiLdGeoProperty::new(NgsiLdGeometry::Point {
            coordinates: [1.5, 2.5].into(),
        });

        assert_eq!(
            geo.value,
            NgsiLdGeometry::Point {
                coordinates: [1.5, 2.5].into(),
            }
        );
        assert!(geo.observed_at.is_none());
        assert!(geo.dataset_id.is_none());
    }
}
