use crate::{cause::Cause, code::diagnostic_code::DiagnosticCode, context_field::ContextField, severity::Severity};
use getset::{CopyGetters, Getters};

/// What makes two failures the same failure.
///
/// This is a separate type from [`Diagnostic`](crate::diagnostic::Diagnostic) rather than a subset of
/// its fields with a hand-written `Hash`, because the distinction has to be unrepresentable rather
/// than merely documented: an entity id in the key is what would flood a terminal with a hundred
/// identical lines, and there is no way to put one here. It carries the severity, the code, the
/// defining context, and the rendered cause chain, nothing that varies between occurrences.
///
/// The derived `Hash` and `Eq` are the whole point: no `educe`, no ignore attributes, no manual
/// implementation that could drift from the field list.
#[derive(Clone, CopyGetters, Debug, Eq, Getters, Hash, PartialEq)]
pub struct DiagnosticIdentity {
    /// How serious the failure is.
    #[getset(get_copy = "pub")]
    severity: Severity,
    /// The failure's stable name.
    #[getset(get_copy = "pub")]
    code: DiagnosticCode,
    /// The context that distinguishes this failure from another, sorted by kind so insertion order
    /// cannot split one key into two.
    #[getset(get = "pub")]
    defining: Vec<ContextField>,
    /// The rendered `source()` chain, so two failures sharing a headline but not a root cause stay
    /// distinct.
    #[getset(get = "pub")]
    causes: Vec<Cause>,
}

impl DiagnosticIdentity {
    /// Assembles an identity, ordering the defining context by field kind.
    ///
    /// Sorting here rather than trusting the caller is what makes the key insensitive to the order a
    /// builder happened to be given its fields in.
    #[must_use]
    pub fn new(severity: Severity, code: DiagnosticCode, defining: Vec<ContextField>, causes: Vec<Cause>) -> DiagnosticIdentity {
        let mut defining = defining;
        defining.sort_by_key(ContextField::kind);
        let defining = defining;

        DiagnosticIdentity {
            severity,
            code,
            defining,
            causes,
        }
    }

    /// The first defining field of the given kind, if the identity carries one.
    #[must_use]
    pub fn field(&self, predicate: impl Fn(&ContextField) -> bool) -> Option<&ContextField> {
        self.defining.iter().find(|field| predicate(field))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        cause::Cause,
        code::{broker_code::BrokerCode, diagnostic_code::DiagnosticCode},
        context_field::ContextField,
        detail::Detail,
        diagnostic_identity::DiagnosticIdentity,
        severity::Severity,
    };
    use http::StatusCode;
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };

    fn hash_of(identity: &DiagnosticIdentity) -> u64 {
        let mut hasher = DefaultHasher::new();
        identity.hash(&mut hasher);
        hasher.finish()
    }

    fn rejection(fields: Vec<ContextField>, causes: Vec<Cause>) -> DiagnosticIdentity {
        DiagnosticIdentity::new(Severity::Error, DiagnosticCode::Broker(BrokerCode::EntityRejected), fields, causes)
    }

    fn status() -> ContextField {
        ContextField::HttpStatus(StatusCode::UNPROCESSABLE_ENTITY)
    }

    fn detail() -> ContextField {
        ContextField::Detail(Detail::new("attribute 'dateObserved' is not a valid DateTime"))
    }

    #[test]
    fn field_insertion_order_does_not_change_the_identity() {
        let one = rejection(vec![status(), detail()], Vec::new());
        let other = rejection(vec![detail(), status()], Vec::new());

        assert_eq!(one, other);
        assert_eq!(hash_of(&one), hash_of(&other));
    }

    #[test]
    fn a_differing_status_is_a_different_identity() {
        let one = rejection(vec![status()], Vec::new());
        let other = rejection(vec![ContextField::HttpStatus(StatusCode::CONFLICT)], Vec::new());

        assert_ne!(one, other);
    }

    #[test]
    fn a_differing_cause_is_a_different_identity() {
        let one = rejection(vec![status()], vec![Cause::new("expected an array")]);
        let other = rejection(vec![status()], vec![Cause::new("unexpected end of input")]);

        assert_ne!(one, other);
    }

    #[test]
    fn a_severity_change_is_a_different_identity() {
        let error = rejection(vec![status()], Vec::new());
        let warning = DiagnosticIdentity::new(
            Severity::Warning,
            DiagnosticCode::Broker(BrokerCode::EntityRejected),
            vec![status()],
            Vec::new(),
        );

        assert_ne!(error, warning);
    }
}
