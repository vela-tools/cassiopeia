use iri_rs::{IriRefBuf, iri_ref};
use serde::Deserialize;

/// The RFC 7807 problem-details body a broker attaches to a failed request or a rejected entity.
///
/// ETSI GS CIM 009 v1.9.1 clause 6.3.3 requires one on every error response, and clause 5.5.3
/// defines `detail` as "a detailed message that should convey enough information about the error",
/// which is why this is parsed for every non-success status rather than only for a 207.
///
/// `type` is an IRI *reference*, not an IRI: RFC 7807 section 3.1.1 defines it as a URI reference, so
/// a relative form is legal and a stricter type would reject a conformant body. It is also optional
/// on the wire (the same section assigns it `about:blank` when absent), so a broker that omits it
/// must not take the whole parse down with it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProblemDetails {
    /// The problem-type identifier, `about:blank` when the body named none.
    #[serde(rename = "type", default = "about_blank")]
    pub problem_type: IriRefBuf,
    /// A short human-readable summary of the problem type.
    #[serde(default)]
    pub title: Option<Box<str>>,
    /// A human-readable explanation specific to this occurrence.
    #[serde(default)]
    pub detail: Option<Box<str>>,
    /// The HTTP status code the broker associates with this problem.
    #[serde(default)]
    pub status: Option<u16>,
    /// The identifier of this specific occurrence, which clause 5.5.3's own example carries.
    #[serde(default)]
    pub instance: Option<IriRefBuf>,
}

/// The problem type RFC 7807 section 3.1.1 assigns when a body names none.
fn about_blank() -> IriRefBuf {
    IriRefBuf::from(iri_ref!("about:blank"))
}

#[cfg(test)]
mod tests {
    use crate::broker::problem_details::ProblemDetails;
    use iri_rs::IriRef;

    #[test]
    fn the_specification_example_parses_every_field() {
        // The shape ETSI GS CIM 009 v1.9.1 clause 5.5.3 gives as its own example.
        let details: ProblemDetails = serde_json::from_str(
            r#"{"type":"https://uri.etsi.org/ngsi-ld/errors/BadRequestData","title":"Bad Request","detail":"attribute 'dateObserved' is not a valid DateTime","status":400,"instance":"urn:ngsi-ld:request:42"}"#,
        )
        .unwrap();

        assert_eq!(details.problem_type.as_str(), "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert_eq!(details.title.as_deref(), Some("Bad Request"));
        assert_eq!(details.detail.as_deref(), Some("attribute 'dateObserved' is not a valid DateTime"));
        assert_eq!(details.status, Some(400));
        assert_eq!(details.instance.as_ref().map(IriRef::as_str), Some("urn:ngsi-ld:request:42"));
    }

    #[test]
    fn a_body_without_a_type_defaults_to_about_blank() {
        let details: ProblemDetails = serde_json::from_str(r#"{"title":"Conflict"}"#).unwrap();

        assert_eq!(details.problem_type.as_str(), "about:blank");
    }

    #[test]
    fn a_detail_only_body_parses() {
        let details: ProblemDetails = serde_json::from_str(r#"{"detail":"entity present"}"#).unwrap();

        assert_eq!(details.detail.as_deref(), Some("entity present"));
        assert_eq!(details.title, None);
    }

    #[test]
    fn a_relative_problem_type_is_accepted() {
        // RFC 7807 section 3.1.1 defines `type` as a URI reference, so a relative form is legal.
        let details: ProblemDetails = serde_json::from_str(r#"{"type":"/errors/BadRequestData"}"#).unwrap();

        assert_eq!(details.problem_type.as_str(), "/errors/BadRequestData");
    }

    #[test]
    fn extension_members_are_ignored() {
        // RFC 7807 section 3.2 lets a problem type define extra members; they are not ours to read.
        let details: ProblemDetails = serde_json::from_str(r#"{"type":"about:blank","balance":30,"accounts":["/a/1"]}"#).unwrap();

        assert_eq!(details.problem_type.as_str(), "about:blank");
    }
}
