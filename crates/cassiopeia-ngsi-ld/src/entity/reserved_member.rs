/// The entity members NGSI-LD reserves, which are therefore never user attributes.
///
/// ETSI GS CIM 009 v1.9.1 clause 4.5.1 makes every attribute a top-level member of the entity
/// alongside these, which is what lets both a schema-tree editor and a validation diagnostic decide
/// whether a top-level member names an attribute simply by asking whether it is reserved.
///
/// `id` and `type` are the two mandatory members; `scope` and `expiresAt` are the optional reserved
/// ones the same clause lists; `dateCreated` and `dateModified` are broker-managed temporal members;
/// and `@context` is the JSON-LD context.
pub const RESERVED_MEMBERS: [&str; 7] = ["id", "type", "scope", "expiresAt", "dateCreated", "dateModified", "@context"];

/// Whether `name` is one of the reserved entity members that are never attributes.
#[must_use]
pub fn is_reserved_member(name: &str) -> bool {
    RESERVED_MEMBERS.contains(&name)
}

#[cfg(test)]
mod tests {
    use crate::entity::reserved_member::{RESERVED_MEMBERS, is_reserved_member};
    use std::collections::HashSet;

    #[test]
    fn the_mandatory_and_managed_members_are_reserved() {
        assert!(is_reserved_member("id"));
        assert!(is_reserved_member("type"));
        assert!(is_reserved_member("@context"));
        assert!(!is_reserved_member("temperature"));
    }

    #[test]
    fn the_optional_reserved_members_of_clause_4_5_1_are_covered() {
        assert!(is_reserved_member("scope"));
        assert!(is_reserved_member("expiresAt"));
    }

    #[test]
    fn every_reserved_member_is_listed_once() {
        assert_eq!(RESERVED_MEMBERS.iter().collect::<HashSet<_>>().len(), RESERVED_MEMBERS.len());
    }
}
