use crate::entity::{
    NgsiLdEntity,
    attribute::{Attributes, NgsiLdAttribute, NgsiLdAttributeWrapper},
    context::NgsiLdContext,
    name::NameBuf,
    scope::NgsiLdScope,
};
use foldhash::fast::RandomState;
use indexmap::{IndexMap, map::Entry};
use urn_rs::Urn;

/// One attribute's occurrences accumulated across every observation of a single entity id.
///
/// An attribute is consistently one kind or the other for a given name: `name` never carries an
/// `observedAt` and stays static, while `maxSustainedWind` always does and forms a series.
enum AggregatedAttribute {
    /// A value with no `observedAt`; the first observation's instance is kept, later ones ignored.
    ///
    /// Boxed to keep the two variants' sizes close, mirroring [`NgsiLdAttributeWrapper::Single`]: the
    /// attribute struct is far larger than the `Temporal` variant's `Vec` handle.
    Static(Box<NgsiLdAttribute>),
    /// A time-stamped series; every observation's instance is appended.
    Temporal(Vec<NgsiLdAttribute>),
}

/// The occurrences accumulated for each attribute name, in first-seen order.
///
/// Keyed by names a mapping declared and probed once per attribute per observation, so it hashes with
/// `foldhash` rather than the standard library's `SipHash`, matching
/// [`Attributes`](crate::entity::attribute::Attributes).
type AggregatedAttributes = IndexMap<NameBuf, AggregatedAttribute, RandomState>;

/// Folds the many single-observation entities that share one id into a single NGSI-LD
/// `EntityTemporal` (ETSI GS CIM 009 v1.9.1, clause 5.2.20).
///
/// Each observation reaches [`push`](TemporalAggregate::push) as its own [`NgsiLdEntity`] carrying
/// one `observedAt`-stamped instance per temporal attribute. [`finish`](TemporalAggregate::finish)
/// emits one entity whose every temporal attribute is a time-ordered array of those instances: the
/// same wire shape a normalized or concise `EntityTemporal` takes. The `@context`, `id`, `type`, and
/// `scope` are taken from the first observation.
pub struct TemporalAggregate {
    /// The `@context` of the first observation, carried onto the folded entity.
    context: Option<NgsiLdContext>,
    /// The shared base id every observation resolves to.
    id: Urn,
    /// The entity type of the first observation.
    entity_type: NameBuf,
    /// The Smart Data Model scope of the first observation, retained for schema resolution.
    scope: Option<NgsiLdScope>,
    /// The accumulating attribute occurrences, in first-seen order.
    attributes: AggregatedAttributes,
}

impl TemporalAggregate {
    /// Seeds the aggregate from the first observation of an entity id.
    #[must_use]
    pub fn new(entity: NgsiLdEntity) -> TemporalAggregate {
        let NgsiLdEntity {
            context,
            id,
            entity_type,
            scope,
            attributes,
        } = entity;
        let mut aggregate = TemporalAggregate {
            context,
            id,
            entity_type,
            scope,
            attributes: AggregatedAttributes::default(),
        };
        aggregate.merge_attributes(attributes);
        aggregate
    }

    /// The shared base id every observation folded into this aggregate resolves to.
    ///
    /// The fold stage reads it to tell whether the next observation continues the open aggregate or
    /// starts a new id.
    #[must_use]
    pub const fn id(&self) -> &Urn {
        &self.id
    }

    /// Merges a further observation of the same id into the aggregate.
    ///
    /// Only the observation's attribute instances are taken; its `@context`, id, type, and scope are
    /// ignored, since those come from the first observation.
    pub fn push(&mut self, entity: NgsiLdEntity) {
        self.merge_attributes(entity.attributes);
    }

    /// Emits the folded entity, each temporal attribute a time-ordered instance array.
    #[must_use]
    pub fn finish(self) -> NgsiLdEntity {
        let mut attributes = Attributes::default();
        for (name, bucket) in self.attributes {
            let wrapper = match bucket {
                AggregatedAttribute::Static(instance) => NgsiLdAttributeWrapper::Single(instance),
                AggregatedAttribute::Temporal(mut instances) => {
                    // Deterministic output: the observations in ascending time order. A batch is
                    // written through `into_par_iter`, so two observations of one id can arrive out of
                    // order and the series cannot be assumed sorted. Near-sorted is the normal
                    // case, and `is_sorted_by_key` rules the sort out with one allocation-free scan.
                    // When a sort is needed, `sort_by_cached_key` permutes an auxiliary key array
                    // instead of merge-moving these large enum values, and stays stable so instances
                    // sharing one `observedAt` keep their arrival order.
                    if !instances.is_sorted_by_key(NgsiLdAttribute::observed_at) {
                        instances.sort_by_cached_key(NgsiLdAttribute::observed_at);
                    }
                    NgsiLdAttributeWrapper::Multi(instances)
                }
            };
            attributes.insert(name, wrapper);
        }
        NgsiLdEntity {
            context: self.context,
            id: self.id,
            entity_type: self.entity_type,
            scope: self.scope,
            attributes,
        }
    }

    /// Folds every instance of one observation's attribute map into the accumulator.
    fn merge_attributes(&mut self, attributes: Attributes) {
        for (name, wrapper) in attributes {
            match wrapper {
                NgsiLdAttributeWrapper::Single(instance) => self.merge_instance(name, *instance),
                NgsiLdAttributeWrapper::Multi(instances) => {
                    for instance in instances {
                        // Each instance of an incoming array lands under the same name; the clone
                        // feeds the shared per-instance merge without consuming the loop key.
                        self.merge_instance(name.clone(), instance);
                    }
                }
            }
        }
    }

    /// Merges a single attribute instance under `name`, routing by whether it is time-stamped.
    fn merge_instance(&mut self, name: NameBuf, instance: NgsiLdAttribute) {
        let temporal = instance.observed_at().is_some();
        match self.attributes.entry(name) {
            Entry::Occupied(mut occupied) => match occupied.get_mut() {
                // A further time-stamped instance extends the series; a stray static instance under
                // the same name is dropped rather than breaking the array shape.
                AggregatedAttribute::Temporal(instances) => {
                    if temporal {
                        instances.push(instance);
                    }
                }
                // A time-stamped instance under a name first seen as static supersedes it: a mixed
                // static/temporal attribute is not a valid shape, so the series form wins.
                AggregatedAttribute::Static(_) => {
                    if temporal {
                        *occupied.get_mut() = AggregatedAttribute::Temporal(vec![instance]);
                    }
                }
            },
            Entry::Vacant(vacant) => {
                let bucket = if temporal {
                    AggregatedAttribute::Temporal(vec![instance])
                } else {
                    AggregatedAttribute::Static(Box::new(instance))
                };
                vacant.insert(bucket);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::{
            NgsiLdEntity,
            attribute::{NgsiLdAttribute, NgsiLdAttributeWrapper, geo_property::NgsiLdGeoProperty, property::NgsiLdProperty},
            name::NameBuf,
            representation::NgsiLdSerializable,
            temporal_aggregate::TemporalAggregate,
        },
        value::types::Value,
    };
    use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
    use cassiopeia_geometry::geometry::NgsiLdGeometry;
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use urn_rs::Urn;

    fn storm_id() -> Urn {
        "urn:ngsi-ld:TropicalCyclone:AL122005".parse::<Urn>().unwrap()
    }

    fn at(rfc3339: &str) -> DateTime<Utc> {
        rfc3339.parse::<DateTime<Utc>>().unwrap()
    }

    /// A one-attribute observation: a temporal `maxSustainedWind` Property stamped at `observed_at`.
    fn wind_observation(value: f64, observed_at: &str) -> NgsiLdEntity {
        let mut property = NgsiLdProperty::new(Value::from(json!(value)));
        property.observed_at = Some(at(observed_at));
        let mut entity = NgsiLdEntity::new(storm_id(), NameBuf::new("TropicalCyclone").unwrap());
        entity.attributes.insert(
            NameBuf::new("maxSustainedWind").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(property)),
        );
        entity
    }

    #[test]
    fn two_temporal_observations_of_one_id_fold_into_a_time_sorted_instance_array() {
        // Pushed newest first, to prove finish sorts rather than preserving insertion order.
        let mut aggregate = TemporalAggregate::new(wind_observation(50.0, "2005-08-25T18:00:00Z"));
        aggregate.push(wind_observation(45.0, "2005-08-25T12:00:00Z"));

        let entity = aggregate.finish();
        assert_eq!(entity.id, storm_id());

        let NgsiLdAttributeWrapper::Multi(instances) = entity.attributes.get("maxSustainedWind").expect("the folded attribute") else {
            panic!("expected a Multi of instances");
        };
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].observed_at(), Some(at("2005-08-25T12:00:00Z")));
        assert_eq!(instances[1].observed_at(), Some(at("2005-08-25T18:00:00Z")));
    }

    /// Every instance's numeric value, in emitted order, for a folded temporal attribute.
    fn wind_series(entity: &NgsiLdEntity) -> Vec<Value> {
        let NgsiLdAttributeWrapper::Multi(instances) = entity.attributes.get("maxSustainedWind").expect("the folded attribute") else {
            panic!("expected a Multi of instances");
        };
        instances
            .iter()
            .map(|instance| {
                let NgsiLdAttribute::Property(property) = instance else {
                    panic!("expected a property instance");
                };
                property.value.clone()
            })
            .collect()
    }

    #[test]
    fn an_already_ascending_series_is_emitted_unchanged() {
        let mut aggregate = TemporalAggregate::new(wind_observation(45.0, "2005-08-25T12:00:00Z"));
        aggregate.push(wind_observation(50.0, "2005-08-25T18:00:00Z"));
        aggregate.push(wind_observation(65.0, "2005-08-26T00:00:00Z"));

        let entity = aggregate.finish();

        assert_eq!(
            wind_series(&entity),
            vec![Value::from(json!(45.0)), Value::from(json!(50.0)), Value::from(json!(65.0))]
        );
    }

    #[test]
    fn instances_sharing_one_observed_at_keep_their_arrival_order() {
        // A stable sort is required: two observations stamped identically carry no ordering of their
        // own, so the order they were folded in is the only defensible one.
        let mut aggregate = TemporalAggregate::new(wind_observation(50.0, "2005-08-25T18:00:00Z"));
        aggregate.push(wind_observation(45.0, "2005-08-25T12:00:00Z"));
        aggregate.push(wind_observation(46.0, "2005-08-25T12:00:00Z"));
        aggregate.push(wind_observation(47.0, "2005-08-25T12:00:00Z"));

        let entity = aggregate.finish();

        assert_eq!(
            wind_series(&entity),
            vec![
                Value::from(json!(45.0)),
                Value::from(json!(46.0)),
                Value::from(json!(47.0)),
                Value::from(json!(50.0))
            ]
        );
    }

    #[test]
    fn a_static_attribute_is_kept_once_as_a_single_instance() {
        // `name` carries no observedAt, so it stays one value across observations rather than an array.
        let name_instance = || NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty::new(Value::from(json!("KATRINA")))));
        let mut first = wind_observation(50.0, "2005-08-25T18:00:00Z");
        first.attributes.insert(NameBuf::new("name").unwrap(), name_instance());
        let mut second = wind_observation(45.0, "2005-08-25T12:00:00Z");
        second.attributes.insert(NameBuf::new("name").unwrap(), name_instance());

        let mut aggregate = TemporalAggregate::new(first);
        aggregate.push(second);
        let entity = aggregate.finish();

        let name = entity.attributes.get("name").expect("the static attribute");
        assert!(matches!(name, NgsiLdAttributeWrapper::Single(_)));
    }

    #[test]
    fn a_temporal_geo_property_folds_into_an_instance_array() {
        fn track_point(lon: f64, lat: f64, observed_at: &str) -> NgsiLdEntity {
            let mut geo = NgsiLdGeoProperty::new(NgsiLdGeometry::Point {
                coordinates: [lon, lat].into(),
            });
            geo.observed_at = Some(at(observed_at));
            let mut entity = NgsiLdEntity::new(storm_id(), NameBuf::new("TropicalCyclone").unwrap());
            entity.attributes.insert(
                NameBuf::new("location").unwrap(),
                NgsiLdAttributeWrapper::single(NgsiLdAttribute::GeoProperty(geo)),
            );
            entity
        }

        let mut aggregate = TemporalAggregate::new(track_point(-75.1, 23.1, "2005-08-25T12:00:00Z"));
        aggregate.push(track_point(-76.2, 24.0, "2005-08-25T18:00:00Z"));
        let entity = aggregate.finish();

        let NgsiLdAttributeWrapper::Multi(instances) = entity.attributes.get("location").expect("the folded location") else {
            panic!("expected a Multi of geo instances");
        };
        assert_eq!(instances.len(), 2);
        assert!(instances.iter().all(|instance| matches!(instance, NgsiLdAttribute::GeoProperty(_))));
    }

    #[test]
    fn the_folded_entity_serializes_each_temporal_attribute_as_an_instance_array() {
        let mut aggregate = TemporalAggregate::new(wind_observation(45.0, "2005-08-25T12:00:00Z"));
        aggregate.push(wind_observation(50.0, "2005-08-25T18:00:00Z"));
        let entity = aggregate.finish();

        let json = entity.to_json(NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip).unwrap();
        let instances = json
            .get("maxSustainedWind")
            .expect("the serialized attribute")
            .as_array()
            .expect("an instance array");

        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].get("type").unwrap(), "Property");
        let first_ts = instances[0].get("observedAt").and_then(|value| value.as_str()).unwrap();
        let second_ts = instances[1].get("observedAt").and_then(|value| value.as_str()).unwrap();
        assert!(first_ts.parse::<DateTime<Utc>>().unwrap() < second_ts.parse::<DateTime<Utc>>().unwrap());
        assert_eq!(instances[0].get("value").unwrap(), &json!(45.0));
        assert_eq!(instances[1].get("value").unwrap(), &json!(50.0));
    }
}
