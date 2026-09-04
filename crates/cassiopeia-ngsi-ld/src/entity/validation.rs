use crate::entity::{
    NgsiLdEntity,
    attribute::{
        NgsiLdAttribute,
        NgsiLdAttributeWrapper,
        geo_property::NgsiLdGeoProperty,
        json_property::NgsiLdJsonProperty,
        language_property::NgsiLdLanguageProperty,
        list_property::NgsiLdListProperty,
        list_relationship::NgsiLdListRelationship,
        property::NgsiLdProperty,
        relationship::NgsiLdRelationship,
        vocab_property::NgsiLdVocabProperty,
    },
    error::{MandatoryMember, NgsiLdError, Result},
};

/// Checks an NGSI-LD value for structural spec compliance.
pub trait Validate {
    /// Validates the value.
    ///
    /// # Errors
    /// Returns [`NgsiLdError`](crate::entity::error::NgsiLdError) when a mandatory field is missing
    /// or a nested attribute is invalid.
    fn validate(&self) -> Result<()>;
}

impl Validate for NgsiLdEntity {
    fn validate(&self) -> Result<()> {
        // Names and URNs are validated by their respective types (NameBuf, Urn)
        for wrapper in self.attributes.values() {
            wrapper.validate()?;
        }
        Ok(())
    }
}

impl Validate for NgsiLdAttributeWrapper {
    fn validate(&self) -> Result<()> {
        match self {
            NgsiLdAttributeWrapper::Single(attr) => attr.validate(),
            NgsiLdAttributeWrapper::Multi(attrs) => {
                for attr in attrs {
                    attr.validate()?;
                }
                Ok(())
            }
        }
    }
}

impl Validate for NgsiLdAttribute {
    fn validate(&self) -> Result<()> {
        match self {
            NgsiLdAttribute::Property(p) => p.validate(),
            NgsiLdAttribute::Relationship(r) => r.validate(),
            NgsiLdAttribute::GeoProperty(g) => g.validate(),
            NgsiLdAttribute::ListRelationship(lr) => lr.validate(),
            NgsiLdAttribute::LanguageProperty(lp) => lp.validate(),
            NgsiLdAttribute::VocabProperty(vp) => vp.validate(),
            NgsiLdAttribute::ListProperty(lp) => lp.validate(),
            NgsiLdAttribute::JsonProperty(jp) => jp.validate(),
        }
    }
}

impl Validate for NgsiLdProperty {
    fn validate(&self) -> Result<()> {
        for wrapper in self.attributes.values() {
            wrapper.validate()?;
        }
        Ok(())
    }
}

impl Validate for NgsiLdRelationship {
    fn validate(&self) -> Result<()> {
        for wrapper in self.attributes.values() {
            wrapper.validate()?;
        }
        Ok(())
    }
}

impl Validate for NgsiLdGeoProperty {
    fn validate(&self) -> Result<()> {
        // The type already rules out a GeometryCollection (ETSI GS CIM 009 v1.9.1 clause 4.7); what
        // is left to check is the structure RFC 7946 clause 3.1 states but the type cannot encode.
        self.value.validate_structure().map_err(NgsiLdError::InvalidGeometry)
    }
}

impl Validate for NgsiLdListRelationship {
    fn validate(&self) -> Result<()> {
        if self.object_list.is_empty() {
            return Err(NgsiLdError::MissingMandatoryField {
                member: MandatoryMember::ObjectList,
            });
        }
        for wrapper in self.attributes.values() {
            wrapper.validate()?;
        }
        Ok(())
    }
}

impl Validate for NgsiLdLanguageProperty {
    fn validate(&self) -> Result<()> {
        if self.language_map.is_empty() {
            return Err(NgsiLdError::MissingMandatoryField {
                member: MandatoryMember::LanguageMap,
            });
        }
        for wrapper in self.attributes.values() {
            wrapper.validate()?;
        }
        Ok(())
    }
}

impl Validate for NgsiLdVocabProperty {
    fn validate(&self) -> Result<()> {
        // `has_vocab` is an `IriBuf`, valid and non-empty by construction, so only nested attributes
        // need checking.
        for wrapper in self.attributes.values() {
            wrapper.validate()?;
        }
        Ok(())
    }
}

impl Validate for NgsiLdListProperty {
    fn validate(&self) -> Result<()> {
        for wrapper in self.attributes.values() {
            wrapper.validate()?;
        }
        Ok(())
    }
}

impl Validate for NgsiLdJsonProperty {
    fn validate(&self) -> Result<()> {
        for wrapper in self.attributes.values() {
            wrapper.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::{
        attribute::{NgsiLdAttribute, NgsiLdAttributeWrapper, list_relationship::NgsiLdListRelationship},
        name::NameBuf,
        validation::Validate,
    };

    #[test]
    fn an_empty_list_relationship_fails_validation() {
        assert!(NgsiLdListRelationship::new(vec![]).validate().is_err());
    }

    #[test]
    fn a_populated_list_relationship_validates() {
        let rel = NgsiLdListRelationship::new(vec!["urn:ngsi-ld:A:1".parse().unwrap()]);
        assert!(rel.validate().is_ok());
    }

    #[test]
    fn a_list_relationship_with_an_invalid_nested_attribute_fails_validation() {
        let mut rel = NgsiLdListRelationship::new(vec!["urn:ngsi-ld:A:1".parse().unwrap()]);
        // A nested empty list relationship is itself invalid (missing objectList), so validation must
        // recurse into the sub-attributes and surface it.
        rel.attributes.insert(
            NameBuf::new("bad").unwrap(),
            Box::new(NgsiLdAttributeWrapper::single(NgsiLdAttribute::ListRelationship(NgsiLdListRelationship::new(
                vec![],
            )))),
        );
        assert!(rel.validate().is_err());
    }

    #[test]
    fn a_list_relationship_with_a_valid_nested_attribute_validates() {
        let mut rel = NgsiLdListRelationship::new(vec!["urn:ngsi-ld:A:1".parse().unwrap()]);
        rel.attributes.insert(
            NameBuf::new("good").unwrap(),
            Box::new(NgsiLdAttributeWrapper::single(NgsiLdAttribute::ListRelationship(NgsiLdListRelationship::new(
                vec!["urn:ngsi-ld:B:1".parse().unwrap()],
            )))),
        );
        assert!(rel.validate().is_ok());
    }
}
