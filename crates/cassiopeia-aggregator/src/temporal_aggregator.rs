use crate::aggregator::Aggregator;
use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, temporal_aggregate::TemporalAggregate};

/// The standard [`Aggregator`]: folds each id's contiguous observations into one `EntityTemporal`
/// (ETSI GS CIM 009 v1.9.1 clause 5.2.20).
///
/// Peak memory is one id's observations, never the whole output: the open aggregate is finished and
/// replaced the moment a new id arrives.
pub struct TemporalAggregator {
    /// The aggregate currently open, holding exactly one id at a time.
    open: Option<TemporalAggregate>,
}

impl TemporalAggregator {
    /// Creates an aggregator with nothing open.
    #[must_use]
    pub const fn new() -> TemporalAggregator {
        TemporalAggregator { open: None }
    }
}

impl Default for TemporalAggregator {
    fn default() -> TemporalAggregator {
        TemporalAggregator::new()
    }
}

impl Aggregator for TemporalAggregator {
    fn offer(&mut self, entity: NgsiLdEntity) -> Option<NgsiLdEntity> {
        // While the incoming id matches the open aggregate, fold it in and emit nothing.
        if matches!(&self.open, Some(aggregate) if aggregate.id() == &entity.id) {
            if let Some(aggregate) = self.open.as_mut() {
                aggregate.push(entity);
            }
            return None;
        }
        // A new id, or the first observation: seed the new aggregate and emit any previous one. The
        // upstream emits an id's observations contiguously, so the replaced aggregate is complete.
        self.open.replace(TemporalAggregate::new(entity)).map(TemporalAggregate::finish)
    }

    fn finish(&mut self) -> Option<NgsiLdEntity> {
        self.open.take().map(TemporalAggregate::finish)
    }
}

#[cfg(test)]
mod tests {
    use crate::{aggregator::Aggregator, temporal_aggregator::TemporalAggregator};
    use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
    use cassiopeia_geometry::geometry::NgsiLdGeometry;
    use cassiopeia_ngsi_ld::{
        entity::{
            NgsiLdEntity,
            attribute::{NgsiLdAttribute, NgsiLdAttributeWrapper, geo_property::NgsiLdGeoProperty, property::NgsiLdProperty},
            name::NameBuf,
            representation::NgsiLdSerializable,
        },
        value::types::Value,
    };
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    fn at(rfc3339: &str) -> DateTime<Utc> {
        rfc3339.parse::<DateTime<Utc>>().unwrap()
    }

    /// A temporal `maxSustainedWind` observation for `id`, stamped at `observed_at`.
    fn wind_observation(id: &str, value: f64, observed_at: &str) -> NgsiLdEntity {
        let mut property = NgsiLdProperty::new(Value::from(json!(value)));
        property.observed_at = Some(at(observed_at));
        let mut entity = NgsiLdEntity::new(urn(id), NameBuf::new("TropicalCyclone").unwrap());
        entity.attributes.insert(
            NameBuf::new("maxSustainedWind").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(property)),
        );
        entity
    }

    /// A static `name` instance carrying no `observedAt`.
    fn name_instance() -> NgsiLdAttributeWrapper {
        NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty::new(Value::from(json!("KATRINA")))))
    }

    /// A temporal `location` `GeoProperty` observation for `id`.
    fn location_observation(id: &str, lon: f64, lat: f64, observed_at: &str) -> NgsiLdEntity {
        let mut geo = NgsiLdGeoProperty::new(NgsiLdGeometry::Point {
            coordinates: [lon, lat].into(),
        });
        geo.observed_at = Some(at(observed_at));
        let mut entity = NgsiLdEntity::new(urn(id), NameBuf::new("TropicalCyclone").unwrap());
        entity.attributes.insert(
            NameBuf::new("location").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::GeoProperty(geo)),
        );
        entity
    }

    #[test]
    fn one_id_with_out_of_order_observations_folds_into_a_time_sorted_multi() {
        let mut aggregator = TemporalAggregator::new();
        // Offered newest first; both continue the one open id, so neither emits.
        assert!(
            aggregator
                .offer(wind_observation("urn:ngsi-ld:TropicalCyclone:A", 50.0, "2005-08-25T18:00:00Z"))
                .is_none()
        );
        assert!(
            aggregator
                .offer(wind_observation("urn:ngsi-ld:TropicalCyclone:A", 45.0, "2005-08-25T12:00:00Z"))
                .is_none()
        );
        let folded = aggregator.finish().expect("the last id flushes");

        // Serialize so the assertion uses the public wire shape; the instances must be a `Multi`
        // ordered ascending by `observedAt` regardless of arrival order.
        let json = folded.to_json(NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip).unwrap();
        let instances = json
            .get("maxSustainedWind")
            .expect("the serialized attribute")
            .as_array()
            .expect("an instance array");
        assert_eq!(instances.len(), 2);
        let first = instances[0].get("observedAt").and_then(serde_json::Value::as_str).unwrap();
        let second = instances[1].get("observedAt").and_then(serde_json::Value::as_str).unwrap();
        assert!(first.parse::<DateTime<Utc>>().unwrap() < second.parse::<DateTime<Utc>>().unwrap());
        assert_eq!(instances[0].get("value").unwrap(), &json!(45.0));
        assert_eq!(instances[1].get("value").unwrap(), &json!(50.0));
    }

    #[test]
    fn a_static_attribute_stays_single_while_the_temporal_one_becomes_a_multi() {
        let mut first = wind_observation("urn:ngsi-ld:TropicalCyclone:A", 50.0, "2005-08-25T18:00:00Z");
        first.attributes.insert(NameBuf::new("name").unwrap(), name_instance());
        let mut second = wind_observation("urn:ngsi-ld:TropicalCyclone:A", 45.0, "2005-08-25T12:00:00Z");
        second.attributes.insert(NameBuf::new("name").unwrap(), name_instance());

        let mut aggregator = TemporalAggregator::new();
        aggregator.offer(first);
        aggregator.offer(second);
        let folded = aggregator.finish().unwrap();

        assert!(matches!(folded.attributes.get("name"), Some(NgsiLdAttributeWrapper::Single(_))));
        assert!(matches!(folded.attributes.get("maxSustainedWind"), Some(NgsiLdAttributeWrapper::Multi(_))));
    }

    #[test]
    fn a_new_id_emits_the_previous_fold_and_the_last_flushes_on_finish() {
        let mut aggregator = TemporalAggregator::new();
        assert!(
            aggregator
                .offer(wind_observation("urn:ngsi-ld:TropicalCyclone:A", 1.0, "2005-08-25T12:00:00Z"))
                .is_none()
        );
        assert!(
            aggregator
                .offer(wind_observation("urn:ngsi-ld:TropicalCyclone:A", 2.0, "2005-08-25T18:00:00Z"))
                .is_none()
        );
        // Id B begins, so id A's fold is emitted here.
        let a = aggregator
            .offer(wind_observation("urn:ngsi-ld:TropicalCyclone:B", 3.0, "2005-08-26T00:00:00Z"))
            .expect("A flushed");
        assert_eq!(a.id, urn("urn:ngsi-ld:TropicalCyclone:A"));
        // Id C begins, so id B's fold is emitted here.
        let b = aggregator
            .offer(wind_observation("urn:ngsi-ld:TropicalCyclone:C", 4.0, "2005-08-27T00:00:00Z"))
            .expect("B flushed");
        assert_eq!(b.id, urn("urn:ngsi-ld:TropicalCyclone:B"));
        // Id C is still open; finish flushes it.
        let c = aggregator.finish().expect("C flushed");
        assert_eq!(c.id, urn("urn:ngsi-ld:TropicalCyclone:C"));
        assert!(aggregator.finish().is_none());
    }

    #[test]
    fn a_geo_series_folds_into_a_multi_of_two() {
        let mut aggregator = TemporalAggregator::new();
        aggregator.offer(location_observation("urn:ngsi-ld:TropicalCyclone:A", -75.1, 23.1, "2005-08-25T12:00:00Z"));
        aggregator.offer(location_observation("urn:ngsi-ld:TropicalCyclone:A", -76.2, 24.0, "2005-08-25T18:00:00Z"));
        let folded = aggregator.finish().unwrap();

        let NgsiLdAttributeWrapper::Multi(instances) = folded.attributes.get("location").expect("the folded location") else {
            panic!("expected a Multi of geo instances");
        };
        assert_eq!(instances.len(), 2);
        assert!(instances.iter().all(|instance| matches!(instance, NgsiLdAttribute::GeoProperty(_))));
    }
}
