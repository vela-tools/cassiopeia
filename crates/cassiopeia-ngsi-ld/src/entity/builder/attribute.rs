use crate::{
    entity::{
        attribute::{
            NestedAttributes,
            NgsiLdAttribute,
            NgsiLdAttributeWrapper,
            geo_property::NgsiLdGeoProperty,
            property::NgsiLdProperty,
            relationship::NgsiLdRelationship,
        },
        dataset_id::DatasetId,
        name::NameBuf,
        value_type::ValueType,
    },
    value::types::Value,
};
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};
use urn_rs::Urn;

/// Builds an [`NgsiLdProperty`] one qualifier at a time.
pub struct PropertyBuilder {
    value: Value,
    value_type: Option<ValueType>,
    observed_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    unit_code: Option<UnitCode>,
    dataset_id: Option<DatasetId>,
    instance_id: Option<Urn>,
    attributes: NestedAttributes,
}

impl PropertyBuilder {
    /// Starts a builder for a property holding `value`, with no qualifiers.
    #[must_use]
    pub fn new(value: impl Into<Value>) -> PropertyBuilder {
        PropertyBuilder {
            value: value.into(),
            value_type: None,
            observed_at: None,
            expires_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: NestedAttributes::default(),
        }
    }

    /// Sets the datatype the mapping declared for the value (ETSI GS CIM 009 v1.9.1 clause 4.5.2).
    #[must_use]
    pub fn value_type(mut self, value_type: ValueType) -> Self {
        self.value_type = Some(value_type);
        self
    }

    /// Sets the instant the value was observed at.
    #[must_use]
    pub const fn observed_at(mut self, observed_at: DateTime<Utc>) -> Self {
        self.observed_at = Some(observed_at);
        self
    }

    /// Sets the instant the value ceases to hold at (ETSI GS CIM 009 v1.9.1 clause 4.22).
    #[must_use]
    pub const fn expires_at(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Sets the UN/CEFACT unit code the value is expressed in.
    #[must_use]
    pub fn unit_code(mut self, unit_code: impl Into<UnitCode>) -> Self {
        self.unit_code = Some(unit_code.into());
        self
    }

    /// Sets the dataset the instance belongs to (ETSI GS CIM 009 v1.9.1 clause 4.5.5.1).
    #[must_use]
    pub fn dataset_id(mut self, dataset_id: DatasetId) -> Self {
        self.dataset_id = Some(dataset_id);
        self
    }

    /// Sets the broker-assigned instance identifier.
    #[must_use]
    pub fn instance_id(mut self, instance_id: Urn) -> Self {
        self.instance_id = Some(instance_id);
        self
    }

    /// Adds a nested sub-attribute under `name`.
    #[must_use]
    pub fn attribute(mut self, name: NameBuf, attribute: NgsiLdAttributeWrapper) -> Self {
        self.attributes.insert(name, Box::new(attribute));
        self
    }

    /// Builds the property.
    #[must_use]
    pub fn build(self) -> NgsiLdProperty {
        NgsiLdProperty {
            value: self.value,
            value_type: self.value_type,
            observed_at: self.observed_at,
            expires_at: self.expires_at,
            unit_code: self.unit_code,
            dataset_id: self.dataset_id,
            instance_id: self.instance_id,
            attributes: self.attributes,
        }
    }
}

/// Builds an [`NgsiLdRelationship`] one qualifier at a time.
pub struct RelationshipBuilder {
    object: Urn,
    object_type: Option<NameBuf>,
    observed_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    dataset_id: Option<DatasetId>,
    instance_id: Option<Urn>,
    attributes: NestedAttributes,
}

impl RelationshipBuilder {
    /// Starts a builder for a relationship pointing at `object`, with no qualifiers.
    #[must_use]
    pub fn new(object: Urn) -> RelationshipBuilder {
        RelationshipBuilder {
            object,
            object_type: None,
            observed_at: None,
            expires_at: None,
            dataset_id: None,
            instance_id: None,
            attributes: NestedAttributes::default(),
        }
    }

    /// Sets the type name of the target entity.
    #[must_use]
    pub fn object_type(mut self, object_type: NameBuf) -> Self {
        self.object_type = Some(object_type);
        self
    }

    /// Sets the instant the relationship was observed at.
    #[must_use]
    pub const fn observed_at(mut self, observed_at: DateTime<Utc>) -> Self {
        self.observed_at = Some(observed_at);
        self
    }

    /// Sets the instant the relationship ceases to hold at (ETSI GS CIM 009 v1.9.1 clause 4.22).
    #[must_use]
    pub const fn expires_at(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Sets the dataset the instance belongs to (ETSI GS CIM 009 v1.9.1 clause 4.5.5.1).
    #[must_use]
    pub fn dataset_id(mut self, dataset_id: DatasetId) -> Self {
        self.dataset_id = Some(dataset_id);
        self
    }

    /// Sets the broker-assigned instance identifier.
    #[must_use]
    pub fn instance_id(mut self, instance_id: Urn) -> Self {
        self.instance_id = Some(instance_id);
        self
    }

    /// Adds a nested sub-attribute under `name`.
    #[must_use]
    pub fn attribute(mut self, name: NameBuf, attribute: NgsiLdAttributeWrapper) -> Self {
        self.attributes.insert(name, Box::new(attribute));
        self
    }

    /// Builds the relationship.
    #[must_use]
    pub fn build(self) -> NgsiLdRelationship {
        NgsiLdRelationship {
            object: self.object,
            object_type: self.object_type,
            observed_at: self.observed_at,
            expires_at: self.expires_at,
            dataset_id: self.dataset_id,
            instance_id: self.instance_id,
            attributes: self.attributes,
        }
    }
}

/// Wraps a built attribute as a single-instance occurrence.
pub trait IntoAttributeWrapper {
    /// The attribute as the one instance of its occurrence.
    fn into_wrapper(self) -> NgsiLdAttributeWrapper;
}

impl IntoAttributeWrapper for NgsiLdProperty {
    fn into_wrapper(self) -> NgsiLdAttributeWrapper {
        NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(self))
    }
}

impl IntoAttributeWrapper for NgsiLdRelationship {
    fn into_wrapper(self) -> NgsiLdAttributeWrapper {
        NgsiLdAttributeWrapper::single(NgsiLdAttribute::Relationship(self))
    }
}

impl IntoAttributeWrapper for NgsiLdGeoProperty {
    fn into_wrapper(self) -> NgsiLdAttributeWrapper {
        NgsiLdAttributeWrapper::single(NgsiLdAttribute::GeoProperty(self))
    }
}
