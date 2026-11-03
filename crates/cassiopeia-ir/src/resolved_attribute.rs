use crate::attribute_content::AttributeContent;
use serde::{Deserialize, Serialize};

/// What one top-level attribute declaration resolved to against a record.
///
/// An attribute that resolved to nothing is not represented at all: it is absent from
/// [`AttributeValues`](crate::entity::AttributeValues).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResolvedAttribute {
    /// A single-instance attribute's content.
    Single(AttributeContent),
    /// A multi-instance attribute's content, one entry per declared instance (ETSI GS CIM 009 v1.9.1
    /// clause 4.5.5).
    ///
    /// Entry `i` belongs to declared instance `i`, the same index its per-instance metadata sits at,
    /// so an instance that resolved to nothing is `None` rather than removed: removing it would move
    /// every later instance onto the wrong `datasetId`.
    Instances(Vec<Option<AttributeContent>>),
}

#[cfg(test)]
mod tests {
    use crate::{attribute_content::AttributeContent, resolved_attribute::ResolvedAttribute};
    use cassiopeia_ngsi_ld::value::types::Value;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_missing_instance_keeps_its_place_through_a_round_trip() {
        let resolved = ResolvedAttribute::Instances(vec![
            Some(AttributeContent::Value(Value::from(1.0))),
            None,
            Some(AttributeContent::Value(Value::from(3.0))),
        ]);
        let decoded: ResolvedAttribute = serde_json::from_str(&serde_json::to_string(&resolved).unwrap()).unwrap();

        assert_eq!(decoded, resolved);
    }
}
