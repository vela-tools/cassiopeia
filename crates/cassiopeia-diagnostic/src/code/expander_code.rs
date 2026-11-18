use strum::{Display, EnumCount, EnumIter};

/// Why an entity a source record describes, or the record as a whole, could not be expanded into
/// fragments.
///
/// Each entity of a record stands on its own, so a code that keeps one entity from being written
/// costs no other entity of the record.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum ExpanderCode {
    /// The record's main entity identity resolved to nothing, so the record names no main entity
    /// and only its synthetic entities were written.
    IdAbsent,
    /// An entity's identity could not be minted into a URN, because its template failed to render,
    /// its entity type cannot form a URN, or a relationship it declares has no `source` to mint
    /// from, so the entity was not written.
    IdUnresolvable,
    /// An entity's `scope` template failed to render for the record, so the entity was not written.
    ScopeUnresolvable,
    /// An entity's `scope` resolved for the record to a value that cannot be the scope of a written
    /// entity: text that is no legal NGSI-LD scope, the NGSI-LD Null, or a value that is not text
    /// (ETSI GS CIM 009 v1.9.1 clause 4.18), so the entity was not written.
    ScopeUnreadable,
    /// The entity `expiresAt` a record resolved to reads as no instant, so the entity publishes
    /// without the member (ETSI GS CIM 009 v1.9.1 clause 4.5.1).
    ExpiresAtUnreadable,
    /// An entity's `expiresAt` template failed to render for a record, so the entity was not
    /// written.
    ExpiresAtUnresolvable,
    /// A relationship's source, or its synthetic entity's identity, failed to render for a record,
    /// so the entity was kept without that link and without the synthetic entity it would have named.
    RelationshipUnresolvable,
    /// The record's source collection is bound to no mapping.
    CollectionUnmatched,
    /// The record carries no source collection, which a collections binding requires.
    CollectionMissing,
}

#[cfg(test)]
mod tests {
    use crate::code::{diagnostic_code::DiagnosticCode, expander_code::ExpanderCode};
    use pretty_assertions::assert_eq;

    #[test]
    fn an_expander_code_renders_a_kebab_case_token() {
        assert_eq!(ExpanderCode::CollectionUnmatched.to_string(), "collection-unmatched");
        assert_eq!(ExpanderCode::ExpiresAtUnreadable.to_string(), "expires-at-unreadable");
        assert_eq!(ExpanderCode::ExpiresAtUnresolvable.to_string(), "expires-at-unresolvable");
        assert_eq!(ExpanderCode::RelationshipUnresolvable.to_string(), "relationship-unresolvable");
    }

    #[test]
    fn each_reason_an_entity_is_not_written_for_its_identity_or_scope_renders_its_own_code() {
        assert_eq!(DiagnosticCode::Expander(ExpanderCode::IdAbsent).to_string(), "expander-id-absent");
        assert_eq!(DiagnosticCode::Expander(ExpanderCode::IdUnresolvable).to_string(), "expander-id-unresolvable");
        assert_eq!(
            DiagnosticCode::Expander(ExpanderCode::ScopeUnresolvable).to_string(),
            "expander-scope-unresolvable"
        );
        assert_eq!(DiagnosticCode::Expander(ExpanderCode::ScopeUnreadable).to_string(), "expander-scope-unreadable");
    }
}
