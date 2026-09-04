use crate::broker::problem_details::ProblemDetails;
use iri_rs::IriBuf;
use serde::Deserialize;

/// The body an NGSI-LD batch endpoint returns on a 207 Multi-Status response.
///
/// ETSI GS CIM 009 v1.9.1 Table 5.2.16-1: `success` is an "Array of valid URIs" naming the entities
/// the broker accepted, and `errors` lists the ones it rejected, each with its own problem details.
///
/// Both members are defaulted because the body is produced by a third party: a broker that accepted
/// everything legitimately sends no `errors` array, and the value the specification assigns to an
/// absent array is the empty one. Refusing the whole body over an omitted array would discard every
/// per-entity diagnostic it carries; reconciliation against the batch that was sent then makes an
/// omission visible rather than silent.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BatchOperationResult {
    /// The identifiers of the entities the broker accepted.
    #[serde(default)]
    pub success: Vec<IriBuf>,
    /// The entities the broker rejected, each with its problem details.
    #[serde(default)]
    pub errors: Vec<BatchEntityError>,
}

/// One rejected entity in a [`BatchOperationResult`].
///
/// ETSI GS CIM 009 v1.9.1 Table 5.2.17-1 defines `entityId`, `error`, and `registrationId`; the last
/// names the context source registration the failure is attributed to, and is what tells a
/// distributed deployment *where* the rejection came from.
///
/// `entityId` is an IRI rather than a URN because clause 4.5.1 requires only that an `id` be a URI,
/// and RFC 3987 section 3.1 makes every URI an IRI, so this can never reject a conformant
/// identifier. Cassiopeia's own minted identifiers stay URNs; it is the broker's echo that must be
/// able to carry anything the specification permits.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchEntityError {
    /// The identifier of the entity that was rejected.
    pub entity_id: IriBuf,
    /// Why the broker rejected it.
    pub error: ProblemDetails,
    /// The context source registration the failure is attributed to, when the broker named one.
    #[serde(default)]
    pub registration_id: Option<IriBuf>,
}

#[cfg(test)]
mod tests {
    use crate::broker::batch_result::BatchOperationResult;
    use iri_rs::Iri;

    #[test]
    fn a_partial_result_separates_the_successes_from_the_errors() {
        let result: BatchOperationResult = serde_json::from_str(
            r#"{"success":["urn:ngsi-ld:A","urn:ngsi-ld:B"],"errors":[{"entityId":"urn:ngsi-ld:C","error":{"type":"urn:ex","title":"bad","status":409}}]}"#,
        )
        .unwrap();

        assert_eq!(result.success.len(), 2);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].entity_id.as_str(), "urn:ngsi-ld:C");
        assert_eq!(result.errors[0].error.status, Some(409));
    }

    #[test]
    fn an_empty_errors_list_reads_as_all_success() {
        let result: BatchOperationResult = serde_json::from_str(r#"{"success":["urn:ngsi-ld:A"],"errors":[]}"#).unwrap();

        assert_eq!(result.success.len(), 1);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn an_omitted_array_reads_as_empty() {
        let accepted: BatchOperationResult = serde_json::from_str(r#"{"success":["urn:ngsi-ld:A"]}"#).unwrap();
        let rejected: BatchOperationResult = serde_json::from_str(r#"{"errors":[{"entityId":"urn:ngsi-ld:C","error":{}}]}"#).unwrap();

        assert!(accepted.errors.is_empty());
        assert!(rejected.success.is_empty());
    }

    #[test]
    fn a_success_member_of_the_wrong_shape_still_errors() {
        // Defaulting an absent array must not become tolerance for a body of the wrong shape.
        assert!(serde_json::from_str::<BatchOperationResult>(r#"{"success":"urn:ngsi-ld:A"}"#).is_err());
    }

    #[test]
    fn a_non_urn_entity_identifier_parses() {
        // Clause 4.5.1 requires only that an `id` be a URI, so an http-scheme identifier is legal.
        let result: BatchOperationResult = serde_json::from_str(r#"{"errors":[{"entityId":"https://example.org/entities/1","error":{}}]}"#).unwrap();

        assert_eq!(result.errors[0].entity_id.as_str(), "https://example.org/entities/1");
    }

    #[test]
    fn the_registration_identifier_is_kept() {
        let result: BatchOperationResult =
            serde_json::from_str(r#"{"errors":[{"entityId":"urn:ngsi-ld:C","error":{},"registrationId":"urn:ngsi-ld:ContextSourceRegistration:7"}]}"#).unwrap();

        assert_eq!(
            result.errors[0].registration_id.as_ref().map(Iri::as_str),
            Some("urn:ngsi-ld:ContextSourceRegistration:7")
        );
    }
}
