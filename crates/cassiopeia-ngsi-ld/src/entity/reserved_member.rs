use crate::entity::name::NameBuf;
use std::str::FromStr;
use strum::{Display, EnumIter, EnumString};

/// The timestamps the Smart Data Models give an entity, which a broker manages and so are not
/// attributes a schema's own properties describe.
const MANAGED_TIMESTAMPS: [&str; 2] = ["dateCreated", "dateModified"];

/// A member ETSI GS CIM 009 v1.9.1 clause 4.5.1 gives an entity beside its attributes.
///
/// The clause writes every attribute as a member of the entity alongside these, so none of them
/// names an attribute.
#[derive(Clone, Copy, Debug, Display, EnumIter, EnumString, Eq, PartialEq)]
#[strum(serialize_all = "camelCase")]
pub enum EntityMember {
    /// The URI identifying the entity.
    Id,
    /// The entity's type name, or names.
    Type,
    /// The entity's scope, or scopes (clause 4.18).
    Scope,
    /// When the entity is to be deleted (clause 4.22).
    ExpiresAt,
    /// The JSON-LD context (clause 4.4).
    #[strum(serialize = "@context")]
    Context,
}

/// A term the NGSI-LD core context defines as a non-reified Property, one written only by its value
/// on an entity or an attribute, beyond the members of an entity itself.
#[derive(Clone, Copy, Debug, Display, EnumIter, EnumString, Eq, PartialEq)]
#[strum(serialize_all = "camelCase")]
pub enum NonReifiedProperty {
    /// What tells apart the instances of one attribute (clause 4.5.5).
    DatasetId,
    /// What identifies one stored instance of an attribute (clause 4.5.7).
    InstanceId,
    /// When an attribute was observed, a Temporal Property (clause 4.8).
    ObservedAt,
    /// When an entity or attribute was entered into a system, a Temporal Property (clause 4.8).
    CreatedAt,
    /// When an entity or attribute was last modified, a Temporal Property (clause 4.8).
    ModifiedAt,
    /// When an entity or attribute was deleted, a Temporal Property (clause 4.8).
    DeletedAt,
    /// When a Snapshot was last used, a Temporal Property (clause 4.8).
    LastUsedAt,
    /// The unit of a Property's value (clause 4.5.2.2), which clause 4.5.5.1 counts among the
    /// non-reified Attributes.
    UnitCode,
    /// The datatype of a Property's value, a non-reified alternative to `@type` (clause 4.5.2.2).
    ValueType,
    /// The language of a value a language filter picked, a non-reified subproperty (clause 4.15).
    Lang,
}

/// A name NGSI-LD keeps from an attribute, and why (ETSI GS CIM 009 v1.9.1 clause 4.5.1).
///
/// It reads as the reason the name is kept, such as ``every entity carries a member named `id` ``.
#[derive(Clone, Copy, Debug, derive_more::Display, Eq, PartialEq)]
pub enum ReservedName {
    /// The name of a member every entity carries beside its attributes, so an attribute of that name
    /// would be written as a second member of the name.
    #[display("every entity carries a member named `{_0}`")]
    EntityMember(EntityMember),
    /// A term the core context defines as a non-reified Property, which clause 4.5.1 keeps from
    /// naming an attribute.
    #[display("the core context defines `{_0}` as a non-reified Property")]
    NonReifiedProperty(NonReifiedProperty),
}

impl ReservedName {
    /// The reservation that keeps `name` from naming an attribute, if any does.
    ///
    /// The match is exact, as JSON-LD matches terms: `ID` and `Type` name attributes.
    #[must_use]
    pub fn of(name: &NameBuf) -> Option<ReservedName> {
        let name = name.as_str();
        EntityMember::from_str(name)
            .map(ReservedName::EntityMember)
            .or_else(|_| NonReifiedProperty::from_str(name).map(ReservedName::NonReifiedProperty))
            .ok()
    }
}

/// Whether `name` is a top-level member of an entity that is never one of its attributes: a member
/// of the entity itself, or a timestamp a broker manages.
///
/// ETSI GS CIM 009 v1.9.1 clause 4.5.1 makes every attribute a top-level member of the entity
/// alongside these, which is what lets both a schema-tree editor and a validation diagnostic decide
/// whether a top-level member names an attribute simply by asking whether it is reserved.
#[must_use]
pub fn is_reserved_member(name: &str) -> bool {
    EntityMember::from_str(name).is_ok() || MANAGED_TIMESTAMPS.contains(&name)
}

#[cfg(test)]
mod tests {
    use crate::entity::{
        name::NameBuf,
        reserved_member::{EntityMember, NonReifiedProperty, ReservedName, is_reserved_member},
    };
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn the_members_of_an_entity_are_spelled_as_clause_4_5_1_writes_them() {
        assert_eq!(
            EntityMember::iter().map(|member| member.to_string()).collect::<Vec<_>>(),
            ["id", "type", "scope", "expiresAt", "@context"]
        );
    }

    #[test]
    fn the_non_reified_properties_are_spelled_as_the_core_context_defines_them() {
        assert_eq!(
            NonReifiedProperty::iter().map(|term| term.to_string()).collect::<Vec<_>>(),
            [
                "datasetId",
                "instanceId",
                "observedAt",
                "createdAt",
                "modifiedAt",
                "deletedAt",
                "lastUsedAt",
                "unitCode",
                "valueType",
                "lang"
            ]
        );
    }

    #[test]
    fn a_member_of_an_entity_is_reserved_as_that_member() {
        for member in [EntityMember::Id, EntityMember::Type, EntityMember::Scope, EntityMember::ExpiresAt] {
            assert_eq!(ReservedName::of(&name(&member.to_string())), Some(ReservedName::EntityMember(member)));
        }
    }

    #[test]
    fn a_non_reified_property_is_reserved_as_that_property() {
        for term in NonReifiedProperty::iter() {
            assert_eq!(ReservedName::of(&name(&term.to_string())), Some(ReservedName::NonReifiedProperty(term)));
        }
    }

    #[test]
    fn a_name_that_differs_only_in_case_or_names_a_reified_term_is_not_reserved() {
        for text in ["ID", "Type", "observedat", "name", "location", "status", "value", "dateCreated", "temperature"] {
            assert_eq!(ReservedName::of(&name(text)), None, "{text}");
        }
    }

    #[test]
    fn a_reservation_reads_as_the_term_and_what_reserves_it() {
        assert_eq!(
            ReservedName::EntityMember(EntityMember::Id).to_string(),
            "every entity carries a member named `id`"
        );
        assert_eq!(
            ReservedName::NonReifiedProperty(NonReifiedProperty::DatasetId).to_string(),
            "the core context defines `datasetId` as a non-reified Property"
        );
    }

    #[test]
    fn the_members_of_an_entity_and_the_managed_timestamps_are_never_attributes() {
        for member in ["id", "type", "scope", "expiresAt", "@context", "dateCreated", "dateModified"] {
            assert!(is_reserved_member(member), "{member}");
        }
        assert!(!is_reserved_member("temperature"));
        assert!(!is_reserved_member("observedAt"));
    }
}
