use crate::{
    cause::Cause,
    code::diagnostic_code::DiagnosticCode,
    context_field::{ContextField, ContextFieldKind},
    diagnostic_identity::DiagnosticIdentity,
    severity::Severity,
};
use getset::{CopyGetters, Getters};
use std::num::NonZeroU64;

/// One reported failure, ready to be rendered, deduplicated, and tallied.
///
/// The headline, the occurrence count, and the incidental context all sit *outside* the identity:
/// a headline that names a count ("rejected 100 entities") must not split the group it belongs to,
/// and neither must the ids of the entities involved.
#[derive(Clone, CopyGetters, Debug, Eq, Getters, PartialEq)]
pub struct Diagnostic {
    /// What makes this failure the same as another.
    #[getset(get = "pub")]
    identity: DiagnosticIdentity,
    /// The one-line summary shown beside the severity glyph.
    headline: Box<str>,
    /// The context that describes this occurrence rather than the failure.
    #[getset(get = "pub")]
    incidental: Vec<ContextField>,
    /// How many occurrences this diagnostic already stands for.
    #[getset(get_copy = "pub")]
    occurrences: NonZeroU64,
}

impl Diagnostic {
    /// Assembles a diagnostic from its parts.
    #[must_use]
    pub const fn new(identity: DiagnosticIdentity, headline: Box<str>, incidental: Vec<ContextField>, occurrences: NonZeroU64) -> Diagnostic {
        Diagnostic {
            identity,
            headline,
            incidental,
            occurrences,
        }
    }

    /// The one-line summary.
    #[must_use]
    pub const fn headline(&self) -> &str {
        &self.headline
    }

    /// How serious the failure is.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.identity.severity()
    }

    /// The failure's stable name.
    #[must_use]
    pub fn code(&self) -> DiagnosticCode {
        self.identity.code()
    }

    /// The context that distinguishes this failure from another.
    #[must_use]
    pub fn defining(&self) -> &[ContextField] {
        self.identity.defining()
    }

    /// The rendered `source()` chain.
    #[must_use]
    pub fn causes(&self) -> &[Cause] {
        self.identity.causes()
    }

    /// The shortest text that explains the failure to a reader.
    ///
    /// A root cause says most, the peer's own `detail` says next-most, and the headline is what is
    /// left. This is what the run summary's reason table shows as the example for a code.
    #[must_use]
    pub fn explanation(&self) -> &str {
        if let Some(cause) = self.causes().first() {
            return cause.as_str();
        }
        match self.defining().iter().find(|field| field.kind() == ContextFieldKind::Detail) {
            Some(ContextField::Detail(detail)) => detail.as_str(),
            Some(_) | None => self.headline(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        code::{broker_code::BrokerCode, diagnostic_code::DiagnosticCode},
        context_field::ContextField,
        detail::Detail,
        diagnostic_builder::DiagnosticBuilder,
        severity::Severity,
    };
    use http::StatusCode;

    fn builder() -> DiagnosticBuilder {
        DiagnosticBuilder::new(
            Severity::Error,
            DiagnosticCode::Broker(BrokerCode::EntityRejected),
            "Broker rejected 100 entities",
        )
    }

    #[test]
    fn the_explanation_prefers_a_root_cause() {
        let diagnostic = builder()
            .with_cause("expected an array")
            .with_context(ContextField::Detail(Detail::new("not a valid DateTime")))
            .build();

        assert_eq!(diagnostic.explanation(), "expected an array");
    }

    #[test]
    fn the_explanation_falls_back_to_the_peers_detail() {
        let diagnostic = builder()
            .with_context(ContextField::HttpStatus(StatusCode::UNPROCESSABLE_ENTITY))
            .with_context(ContextField::Detail(Detail::new("not a valid DateTime")))
            .build();

        assert_eq!(diagnostic.explanation(), "not a valid DateTime");
    }

    #[test]
    fn the_explanation_falls_back_to_the_headline() {
        assert_eq!(builder().build().explanation(), "Broker rejected 100 entities");
    }
}
