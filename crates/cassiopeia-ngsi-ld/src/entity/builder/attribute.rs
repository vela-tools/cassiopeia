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
        name::NameBuf,
    },
    value::types::Value,
};
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};
use urn_rs::Urn;

pub struct PropertyBuilder {
    value: Value,
    observed_at: Option<DateTime<Utc>>,
    unit_code: Option<UnitCode>,
    dataset_id: Option<Urn>,
    instance_id: Option<Urn>,
    attributes: NestedAttributes,
}

impl PropertyBuilder {
    pub fn new(value: impl Into<Value>) -> Self {
        Self {
            value: value.into(),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: NestedAttributes::default(),
        }
    }

    #[must_use]
    pub const fn observed_at(mut self, observed_at: DateTime<Utc>) -> Self {
        self.observed_at = Some(observed_at);
        self
    }

    #[must_use]
    pub fn unit_code(mut self, unit_code: impl Into<UnitCode>) -> Self {
        self.unit_code = Some(unit_code.into());
        self
    }

    #[must_use]
    pub fn dataset_id(mut self, dataset_id: Urn) -> Self {
        self.dataset_id = Some(dataset_id);
        self
    }

    #[must_use]
    pub fn instance_id(mut self, instance_id: Urn) -> Self {
        self.instance_id = Some(instance_id);
        self
    }

    #[must_use]
    pub fn attribute(mut self, name: NameBuf, attribute: NgsiLdAttributeWrapper) -> Self {
        self.attributes.insert(name, Box::new(attribute));
        self
    }

    #[must_use]
    pub fn build(self) -> NgsiLdProperty {
        NgsiLdProperty {
            value: self.value,
            observed_at: self.observed_at,
            unit_code: self.unit_code,
            dataset_id: self.dataset_id,
            instance_id: self.instance_id,
            attributes: self.attributes,
        }
    }
}

pub struct RelationshipBuilder {
    object: Urn,
    object_type: Option<NameBuf>,
    observed_at: Option<DateTime<Utc>>,
    dataset_id: Option<Urn>,
    instance_id: Option<Urn>,
    attributes: NestedAttributes,
}

impl RelationshipBuilder {
    #[must_use]
    pub fn new(object: Urn) -> Self {
        Self {
            object,
            object_type: None,
            observed_at: None,
            dataset_id: None,
            instance_id: None,
            attributes: NestedAttributes::default(),
        }
    }

    #[must_use]
    pub fn object_type(mut self, object_type: NameBuf) -> Self {
        self.object_type = Some(object_type);
        self
    }

    #[must_use]
    pub const fn observed_at(mut self, observed_at: DateTime<Utc>) -> Self {
        self.observed_at = Some(observed_at);
        self
    }

    #[must_use]
    pub fn dataset_id(mut self, dataset_id: Urn) -> Self {
        self.dataset_id = Some(dataset_id);
        self
    }

    #[must_use]
    pub fn instance_id(mut self, instance_id: Urn) -> Self {
        self.instance_id = Some(instance_id);
        self
    }

    #[must_use]
    pub fn attribute(mut self, name: NameBuf, attribute: NgsiLdAttributeWrapper) -> Self {
        self.attributes.insert(name, Box::new(attribute));
        self
    }

    #[must_use]
    pub fn build(self) -> NgsiLdRelationship {
        NgsiLdRelationship {
            object: self.object,
            object_type: self.object_type,
            observed_at: self.observed_at,
            dataset_id: self.dataset_id,
            instance_id: self.instance_id,
            attributes: self.attributes,
        }
    }
}

pub trait IntoAttributeWrapper {
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
