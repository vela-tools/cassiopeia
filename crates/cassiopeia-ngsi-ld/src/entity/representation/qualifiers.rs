//! Shared streaming writers for the common attribute qualifiers, so every representation serializes
//! `valueType` / `observedAt` / `expiresAt` / `unitCode` / `datasetId` / `instanceId` / `objectType` and nested
//! attributes the same way instead of repeating the insertion blocks. Each writes straight into an
//! open [`SerializeMap`] with no intermediate `serde_json::Value`.

use crate::{
    entity::{
        attribute::NestedAttributes,
        name::NameBuf,
        representation::{DisplayStr, ReprAdapter},
    },
    value::{temporal_text, types::Value},
};
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};
use serde::ser::SerializeMap;
use urn_rs::Urn;

/// Writes `valueType` when the Property's value is a time value.
///
/// Clause 4.5.2 makes `valueType` the non-reified datatype of a Property's value, and Annex C.6 uses
/// it to mark a `DateTime`, `Date` or `Time`. Without it the value is a plain string, which a broker
/// stores as text, so a query with a time operand does not match it.
///
/// # Errors
/// Returns the serializer's error if the data type cannot be serialized.
pub(crate) fn serialize_value_type<M: SerializeMap>(map: &mut M, value: &Value) -> Result<(), M::Error> {
    if let Some(value_type) = value.temporal_type() {
        map.serialize_entry("valueType", &value_type)?;
    }
    Ok(())
}

/// Writes `observedAt` when present, as an NGSI-LD `DateTime` (clause 4.8).
///
/// # Errors
/// Returns the serializer's error if the timestamp cannot be serialized.
pub(crate) fn serialize_observed_at<M: SerializeMap>(map: &mut M, observed_at: Option<&DateTime<Utc>>) -> Result<(), M::Error> {
    if let Some(observed_at) = observed_at {
        map.serialize_entry("observedAt", &DisplayStr(temporal_text::date_time(*observed_at)))?;
    }
    Ok(())
}

/// Writes `expiresAt` when present, as an NGSI-LD `DateTime` (clause 4.22).
///
/// # Errors
/// Returns the serializer's error if the timestamp cannot be serialized.
pub(crate) fn serialize_expires_at<M: SerializeMap>(map: &mut M, expires_at: Option<&DateTime<Utc>>) -> Result<(), M::Error> {
    if let Some(expires_at) = expires_at {
        map.serialize_entry("expiresAt", &DisplayStr(temporal_text::date_time(*expires_at)))?;
    }
    Ok(())
}

/// Writes `unitCode` when present.
///
/// # Errors
/// Returns the serializer's error if the unit code cannot be serialized.
pub(crate) fn serialize_unit_code<M: SerializeMap>(map: &mut M, unit_code: Option<&UnitCode>) -> Result<(), M::Error> {
    if let Some(unit_code) = unit_code {
        map.serialize_entry("unitCode", &DisplayStr(unit_code))?;
    }
    Ok(())
}

/// Writes `datasetId` when present.
///
/// # Errors
/// Returns the serializer's error if the dataset id cannot be serialized.
pub(crate) fn serialize_dataset_id<M: SerializeMap>(map: &mut M, dataset_id: Option<&Urn>) -> Result<(), M::Error> {
    if let Some(dataset_id) = dataset_id {
        map.serialize_entry("datasetId", &DisplayStr(dataset_id))?;
    }
    Ok(())
}

/// Writes `instanceId` when present.
///
/// # Errors
/// Returns the serializer's error if the instance id cannot be serialized.
pub(crate) fn serialize_instance_id<M: SerializeMap>(map: &mut M, instance_id: Option<&Urn>) -> Result<(), M::Error> {
    if let Some(instance_id) = instance_id {
        map.serialize_entry("instanceId", &DisplayStr(instance_id))?;
    }
    Ok(())
}

/// Writes `objectType` when present.
///
/// # Errors
/// Returns the serializer's error if the object type cannot be serialized.
pub(crate) fn serialize_object_type<M: SerializeMap>(map: &mut M, object_type: Option<&NameBuf>) -> Result<(), M::Error> {
    if let Some(object_type) = object_type {
        map.serialize_entry("objectType", &DisplayStr(object_type))?;
    }
    Ok(())
}

/// Writes every nested sub-attribute, serialized in the given representation, skipping instances that
/// null-handling drops.
///
/// # Errors
/// Returns the serializer's error if a nested attribute cannot be serialized.
pub(crate) fn serialize_nested<M: SerializeMap>(
    map: &mut M,
    attributes: &NestedAttributes,
    representation: NgsiLdRepresentation,
    skip_null: NgsiLdSkipNull,
) -> Result<(), M::Error> {
    for (name, wrapper) in attributes {
        if wrapper.is_skipped(skip_null) {
            continue;
        }
        map.serialize_entry(name.as_str(), &ReprAdapter::new(wrapper.as_ref(), representation, skip_null))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::entity::{
        attribute::{NestedAttributes, NgsiLdAttribute, NgsiLdAttributeWrapper, property::NgsiLdProperty},
        name::NameBuf,
        representation::{ReprAdapter, qualifiers::serialize_nested},
    };
    use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
    use indexmap::IndexMap;
    use pretty_assertions::assert_eq;
    use serde::{Serialize, Serializer, ser::SerializeMap};
    use serde_json::Value;

    // Serializes a single nested attribute through `serialize_nested` and returns the resulting map.
    struct NestedOnly(NestedAttributes);

    impl Serialize for NestedOnly {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(None)?;
            serialize_nested(&mut map, &self.0, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Include)?;
            map.end()
        }
    }

    #[test]
    fn a_nested_attribute_serializes_under_its_name() {
        let mut nested = IndexMap::default();
        nested.insert(
            NameBuf::new("accuracy").unwrap(),
            Box::new(NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty::new(0.5)))),
        );

        let value = serde_json::to_value(NestedOnly(nested)).unwrap();
        assert_eq!(value.get("accuracy"), Some(&Value::from(0.5)));
    }

    #[test]
    fn a_repr_adapter_streams_the_same_bytes_serde_json_would_build() {
        let property = NgsiLdProperty::new(42);
        let adapter = ReprAdapter::new(&property, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Include);
        assert_eq!(serde_json::to_string(&adapter).unwrap(), "42");
    }
}
