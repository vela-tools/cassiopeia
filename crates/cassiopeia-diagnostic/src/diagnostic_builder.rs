use crate::{
    cause::{Cause, causes_of},
    code::diagnostic_code::DiagnosticCode,
    context_field::ContextField,
    diagnostic::Diagnostic,
    diagnostic_identity::DiagnosticIdentity,
    field_role::FieldRole,
    severity::Severity,
};
use std::{error::Error, num::NonZeroU64};

/// Assembles a [`Diagnostic`], routing each context field to the identity or to the occurrence by
/// the field's own [`role`](ContextField::role).
///
/// The routing is not the caller's decision: a caller that could put an entity id into the identity
/// would flood the terminal with a line per entity, so the builder decides and the caller only says
/// what it knows.
pub struct DiagnosticBuilder {
    /// How serious the failure is.
    severity: Severity,
    /// The failure's stable name.
    code: DiagnosticCode,
    /// The one-line summary.
    headline: Box<str>,
    /// The context that will form the identity.
    defining: Vec<ContextField>,
    /// The context that describes this occurrence.
    incidental: Vec<ContextField>,
    /// The rendered `source()` chain.
    causes: Vec<Cause>,
    /// How many occurrences the finished diagnostic stands for.
    occurrences: NonZeroU64,
}

impl DiagnosticBuilder {
    /// Starts a diagnostic with its severity, code, and headline.
    #[must_use]
    pub fn new(severity: Severity, code: DiagnosticCode, headline: impl Into<Box<str>>) -> DiagnosticBuilder {
        DiagnosticBuilder {
            severity,
            code,
            headline: headline.into(),
            defining: Vec::new(),
            incidental: Vec::new(),
            causes: Vec::new(),
            occurrences: NonZeroU64::MIN,
        }
    }

    /// Attaches one typed fact.
    #[must_use]
    pub fn with_context(mut self, field: ContextField) -> DiagnosticBuilder {
        match field.role() {
            FieldRole::Defining => self.defining.push(field),
            FieldRole::Incidental => self.incidental.push(field),
        }
        self
    }

    /// Attaches a fact that the caller could not always produce.
    #[must_use]
    pub fn with_optional_context(self, field: Option<ContextField>) -> DiagnosticBuilder {
        match field {
            Some(field) => self.with_context(field),
            None => self,
        }
    }

    /// Attaches one rendered link of a cause chain.
    #[must_use]
    pub fn with_cause(mut self, cause: impl Into<Box<str>>) -> DiagnosticBuilder {
        self.causes.push(Cause::new(cause));
        self
    }

    /// Attaches an already-walked cause chain.
    #[must_use]
    pub fn with_causes(mut self, causes: Vec<Cause>) -> DiagnosticBuilder {
        self.causes = causes;
        self
    }

    /// Attaches an error as the reason for this diagnostic: its own message becomes the first cause
    /// and its `source()` chain follows.
    ///
    /// This is the counterpart to [`from_error`], which takes an error's message *as* the headline.
    /// Use it where the headline says what Cassiopeia was doing and the failure belongs underneath:
    /// "cannot resolve the @context" over "no such file or directory".
    #[must_use]
    pub fn because(mut self, error: &dyn Error) -> DiagnosticBuilder {
        self.causes.push(Cause::new(error.to_string()));
        self.causes.extend(causes_of(error));
        self
    }

    /// Declares how many occurrences this one diagnostic already stands for.
    #[must_use]
    pub const fn with_occurrences(mut self, occurrences: NonZeroU64) -> DiagnosticBuilder {
        self.occurrences = occurrences;
        self
    }

    /// Finishes the diagnostic.
    #[must_use]
    pub fn build(self) -> Diagnostic {
        Diagnostic::new(
            DiagnosticIdentity::new(self.severity, self.code, self.defining, self.causes),
            self.headline,
            self.incidental,
            self.occurrences,
        )
    }
}

/// Builds a diagnostic from a typed error, taking its `Display` as the headline and its `source()`
/// chain as the causes.
///
/// This is the single place the workspace walks a cause chain, so a failure reported before the
/// reporter exists and one reported through it carry exactly the same rows.
#[must_use]
pub fn from_error(severity: Severity, code: DiagnosticCode, error: &dyn Error) -> DiagnosticBuilder {
    DiagnosticBuilder::new(severity, code, error.to_string()).with_causes(causes_of(error))
}

#[cfg(test)]
mod tests {
    use crate::{
        code::{broker_code::BrokerCode, diagnostic_code::DiagnosticCode},
        context_field::ContextField,
        detail::Detail,
        diagnostic_builder::{DiagnosticBuilder, from_error},
        severity::Severity,
    };
    use cassiopeia_common::error::io::{IoAction, IoError};
    use iri_rs::IriBuf;
    use std::{io::Error, num::NonZeroU64, path::PathBuf};

    fn code() -> DiagnosticCode {
        DiagnosticCode::Broker(BrokerCode::EntityRejected)
    }

    fn entities(id: &str) -> ContextField {
        ContextField::Entities {
            first: IriBuf::new(id.to_owned()).unwrap(),
            additional: 0,
        }
    }

    #[test]
    fn two_rejections_of_different_entities_share_one_identity() {
        let one = DiagnosticBuilder::new(Severity::Error, code(), "rejected")
            .with_context(ContextField::Detail(Detail::new("bad date")))
            .with_context(entities("urn:ngsi-ld:A:1"))
            .build();
        let other = DiagnosticBuilder::new(Severity::Error, code(), "rejected")
            .with_context(ContextField::Detail(Detail::new("bad date")))
            .with_context(entities("urn:ngsi-ld:A:2"))
            .build();

        assert_eq!(one.identity(), other.identity());
    }

    #[test]
    fn an_incidental_field_never_reaches_the_identity() {
        let diagnostic = DiagnosticBuilder::new(Severity::Error, code(), "rejected")
            .with_context(entities("urn:ngsi-ld:A:1"))
            .build();

        assert!(diagnostic.defining().is_empty());
        assert_eq!(diagnostic.incidental().len(), 1);
    }

    #[test]
    fn a_differing_headline_does_not_split_one_identity() {
        let one = DiagnosticBuilder::new(Severity::Error, code(), "rejected 100 entities").build();
        let other = DiagnosticBuilder::new(Severity::Error, code(), "rejected 42 entities").build();

        assert_eq!(one.identity(), other.identity());
    }

    #[test]
    fn an_absent_optional_field_changes_nothing() {
        let diagnostic = DiagnosticBuilder::new(Severity::Error, code(), "rejected").with_optional_context(None).build();

        assert!(diagnostic.defining().is_empty());
        assert!(diagnostic.incidental().is_empty());
    }

    #[test]
    fn a_diagnostic_starts_at_one_occurrence_and_can_declare_more() {
        let one = DiagnosticBuilder::new(Severity::Error, code(), "rejected").build();
        let many = DiagnosticBuilder::new(Severity::Error, code(), "rejected")
            .with_occurrences(NonZeroU64::new(42).unwrap())
            .build();

        assert_eq!(one.occurrences().get(), 1);
        assert_eq!(many.occurrences().get(), 42);
    }

    #[test]
    fn an_error_attached_as_a_reason_leads_the_cause_chain() {
        let failure = IoError::FileOperation {
            source: Error::other("permission denied"),
            path: PathBuf::from("/etc/context.jsonld"),
            action: IoAction::Read,
        };

        let diagnostic = DiagnosticBuilder::new(Severity::Warning, code(), "Cannot resolve the @context")
            .because(&failure)
            .build();

        assert_eq!(diagnostic.headline(), "Cannot resolve the @context");
        assert_eq!(diagnostic.causes().len(), 2);
        assert!(diagnostic.causes()[0].as_str().contains("context.jsonld"));
        assert_eq!(diagnostic.causes()[1].as_str(), "permission denied");
    }

    #[test]
    fn an_error_becomes_a_headline_plus_its_chain() {
        let failure = IoError::FileOperation {
            source: Error::other("permission denied"),
            path: PathBuf::from("/out/AirQualityObserved.json"),
            action: IoAction::Write,
        };

        let diagnostic = from_error(Severity::Error, code(), &failure).build();

        assert_eq!(diagnostic.headline(), failure.to_string());
        assert!(diagnostic.headline().contains("AirQualityObserved.json"));
        assert_eq!(diagnostic.causes().len(), 1);
        assert_eq!(diagnostic.causes()[0].as_str(), "permission denied");
    }
}
