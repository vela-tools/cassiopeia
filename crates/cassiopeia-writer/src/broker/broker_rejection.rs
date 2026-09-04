use crate::broker::problem_details::ProblemDetails;

/// Which part of a problem body carries the sentence a reader needs.
///
/// RFC 7807 orders these by specificity: `detail` explains *this* occurrence, `title` names the
/// problem type in prose, and the type identifier itself is what is left when the broker sent
/// nothing human-readable. Naming the three makes the precedence a value rather than a chain of
/// `unwrap_or`s, and means a body carrying only a `detail` is reported with that detail instead of
/// with a placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionHeadline<'a> {
    /// The occurrence-specific explanation.
    Detail(&'a str),
    /// The problem type's prose name.
    Title(&'a str),
    /// Only the problem-type identifier was given.
    Type,
}

/// A broker's refusal, in the terms the broker itself used.
///
/// This is the domain value the writer reports; [`ProblemDetails`] is the wire shape it is parsed
/// from, and stays a deserialization concern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerRejection {
    /// The problem body the broker sent.
    problem: ProblemDetails,
}

impl BrokerRejection {
    /// Wraps a parsed problem body.
    #[must_use]
    pub const fn new(problem: ProblemDetails) -> BrokerRejection {
        BrokerRejection { problem }
    }

    /// The problem body.
    #[must_use]
    pub const fn problem(&self) -> &ProblemDetails {
        &self.problem
    }

    /// Which part of the body carries the explanation.
    #[must_use]
    pub fn headline(&self) -> RejectionHeadline<'_> {
        if let Some(detail) = self.problem.detail.as_deref() {
            return RejectionHeadline::Detail(detail);
        }
        match self.problem.title.as_deref() {
            Some(title) => RejectionHeadline::Title(title),
            None => RejectionHeadline::Type,
        }
    }

    /// The explanation as text, whichever part of the body it came from.
    #[must_use]
    pub fn text(&self) -> &str {
        match self.headline() {
            RejectionHeadline::Detail(detail) => detail,
            RejectionHeadline::Title(title) => title,
            RejectionHeadline::Type => self.problem.problem_type.as_str(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::broker::{
        broker_rejection::{BrokerRejection, RejectionHeadline},
        problem_details::ProblemDetails,
    };

    fn rejection(body: &str) -> BrokerRejection {
        BrokerRejection::new(serde_json::from_str::<ProblemDetails>(body).unwrap())
    }

    #[test]
    fn a_detail_wins_over_a_title() {
        let rejection = rejection(r#"{"type":"urn:ex","title":"Bad Request","detail":"not a valid DateTime"}"#);

        assert_eq!(rejection.headline(), RejectionHeadline::Detail("not a valid DateTime"));
        assert_eq!(rejection.text(), "not a valid DateTime");
    }

    #[test]
    fn a_title_is_used_when_there_is_no_detail() {
        let rejection = rejection(r#"{"type":"urn:ex","title":"Already Exists"}"#);

        assert_eq!(rejection.headline(), RejectionHeadline::Title("Already Exists"));
        assert_eq!(rejection.text(), "Already Exists");
    }

    #[test]
    fn a_bare_type_is_reported_as_itself_rather_than_as_a_placeholder() {
        let rejection = rejection(r#"{"type":"https://uri.etsi.org/ngsi-ld/errors/BadRequestData"}"#);

        assert_eq!(rejection.headline(), RejectionHeadline::Type);
        assert_eq!(rejection.text(), "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }

    #[test]
    fn a_detail_only_body_never_reads_as_unspecified() {
        assert_eq!(rejection(r#"{"detail":"entity present"}"#).text(), "entity present");
    }
}
