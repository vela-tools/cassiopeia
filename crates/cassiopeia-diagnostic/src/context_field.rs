use crate::{detail::Detail, json_pointer::JsonPointer, schema_keyword::SchemaKeyword};
use cassiopeia_common::captured_body::CapturedBody;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use http::StatusCode;
use iri_rs::IriBuf;
use std::path::PathBuf;
use strum::EnumDiscriminants;
use url::Url;
use urn_rs::Urn;

/// One typed fact attached to a diagnostic.
///
/// Every payload is a domain type rather than text, so a field cannot be confused with a message and
/// the renderer never has to guess what it is looking at. Which fields take part in a diagnostic's
/// identity is decided by [`ContextField::role`](crate::field_role), not by the caller.
#[derive(Clone, Debug, EnumDiscriminants, Eq, Hash, PartialEq)]
#[strum_discriminants(name(ContextFieldKind))]
#[strum_discriminants(derive(Hash, Ord, PartialOrd, strum::EnumCount, strum::EnumIter))]
pub enum ContextField {
    /// The HTTP status a peer answered with.
    HttpStatus(StatusCode),
    /// The URL a request was addressed to.
    Endpoint(Url),
    /// The RFC 7807 problem-type identifier a peer named. It is a URI *reference* because RFC 7807
    /// section 3.1.1 permits a relative form, which a plain IRI would reject.
    ProblemType(iri_rs::IriRefBuf),
    /// The NGSI-LD entity type involved.
    EntityType(NameBuf),
    /// The attribute involved.
    Attribute(NameBuf),
    /// Where inside the instance a schema violation sits.
    InstancePath(JsonPointer),
    /// Where inside the schema the violated rule lives.
    SchemaPath(JsonPointer),
    /// The JSON Schema keyword that raised a violation.
    Keyword(SchemaKeyword),
    /// The peer's own explanation of the failure.
    Detail(Detail),
    /// The entities a diagnostic covers: the first one named, the rest counted.
    Entities {
        /// The first entity's identifier.
        first: IriBuf,
        /// How many further entities the diagnostic covers.
        additional: u64,
    },
    /// The source file involved.
    SourcePath(PathBuf),
    /// Which retry attempt this was, out of how many.
    Attempt {
        /// The 1-based attempt number.
        attempt: u64,
        /// The attempt ceiling.
        limit: u64,
    },
    /// How large the request body was, in bytes.
    PayloadBytes(u64),
    /// The peer's response body, captured for echoing.
    BodyEcho(CapturedBody),
    /// The registration a broker attributed an entity's outcome to (ETSI GS CIM 009 v1.9.1 Table
    /// 5.2.17-1).
    RegistrationId(IriBuf),
    /// How many entities the batch carried.
    BatchSize(u64),
}

impl ContextField {
    /// Which kind of field this is.
    #[must_use]
    pub fn kind(&self) -> ContextFieldKind {
        ContextFieldKind::from(self)
    }
}

/// Renders a Cassiopeia-minted URN as the identifier a diagnostic carries.
///
/// Broker-echoed identifiers may be any URI (ETSI GS CIM 009 v1.9.1 clause 4.5.1: an `id` "shall be
/// a URI"), while Cassiopeia mints only URNs, so both meet as an IRI: RFC 3987 section 3.1 makes
/// every URI an IRI. `urn_rs::Urn` only constructs from RFC 8141 syntax, a strict subset, so the
/// conversion cannot fail in practice; it is fallible in type only, and an identifier that somehow
/// did not convert is reported without its id rather than not reported at all.
#[must_use]
pub fn minted_id(urn: &Urn) -> Option<IriBuf> {
    IriBuf::new(urn.to_string()).ok()
}

#[cfg(test)]
mod tests {
    use crate::{
        context_field::{ContextField, ContextFieldKind, minted_id},
        detail::Detail,
    };
    use http::StatusCode;
    use urn_rs::Urn;

    #[test]
    fn a_field_reports_its_own_kind() {
        assert_eq!(ContextField::HttpStatus(StatusCode::UNPROCESSABLE_ENTITY).kind(), ContextFieldKind::HttpStatus);
        assert_eq!(ContextField::Detail(Detail::new("bad")).kind(), ContextFieldKind::Detail);
    }

    #[test]
    fn a_minted_urn_converts_into_an_identifier() {
        let urn: Urn = "urn:ngsi-ld:AirQualityObserved:LJ-001".parse().unwrap();

        assert_eq!(minted_id(&urn).unwrap().as_str(), "urn:ngsi-ld:AirQualityObserved:LJ-001");
    }
}
