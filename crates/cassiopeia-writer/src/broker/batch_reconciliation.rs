use crate::broker::batch_result::BatchOperationResult;
use cassiopeia_diagnostic::context_field::minted_id;
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use getset::{CopyGetters, Getters};
use iri_rs::IriBuf;

/// What a 207 Multi-Status body actually accounted for, checked against the batch that was sent.
#[derive(Debug, CopyGetters, Getters, PartialEq, Eq)]
pub struct Reconciliation {
    /// How many entities the broker is credited with writing, never more than were sent.
    #[getset(get_copy = "pub")]
    written: usize,
    /// How many entities were not written, including any the broker never mentioned.
    #[getset(get_copy = "pub")]
    failed: usize,
    /// The entities the body named in neither list, so their outcome is unknown.
    #[getset(get = "pub")]
    unaccounted: Vec<IriBuf>,
}

/// Checks a 207 body against the batch it answers, so the writer's totals reflect what was sent.
///
/// The healthy case pays one integer comparison: a body whose two lists cover the batch exactly is
/// taken as it stands. Only a disagreement costs a diff, and the diff compares identifiers as IRIs,
/// whose equality is RFC 3986 section 6.2.2 syntax-based normalization, which is what clause 5.5.1
/// requires of an NGSI-LD identifier comparison, so a broker echoing an equivalent-but-differently
/// written form still reconciles.
///
/// An entity the broker never mentioned is counted as failed, not as written: its outcome is
/// unknown, and a data producer that guesses in its own favour reports data it may never have
/// delivered. Over-reporting is clamped for the same reason: `written` can never exceed the batch.
#[must_use]
pub fn reconcile(result: &BatchOperationResult, entities: &[NgsiLdEntity]) -> Reconciliation {
    let total = entities.len();
    if result.success.len() + result.errors.len() == total {
        return Reconciliation {
            written: result.success.len(),
            failed: result.errors.len(),
            unaccounted: Vec::new(),
        };
    }

    let unaccounted: Vec<IriBuf> = entities
        .iter()
        .filter_map(|entity| minted_id(&entity.id))
        .filter(|id| !result.success.contains(id) && !result.errors.iter().any(|error| error.entity_id == *id))
        .collect();

    let failed = result.errors.len().saturating_add(unaccounted.len()).min(total);
    Reconciliation {
        written: result.success.len().min(total.saturating_sub(failed)),
        failed,
        unaccounted,
    }
}

#[cfg(test)]
mod tests {
    use crate::broker::{batch_reconciliation::reconcile, batch_result::BatchOperationResult};
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use iri_rs::Iri;
    use urn_rs::Urn;

    fn entities(ids: &[&str]) -> Vec<NgsiLdEntity> {
        ids.iter()
            .map(|id| NgsiLdEntity::new(id.parse::<Urn>().unwrap(), NameBuf::new("Thing").unwrap()))
            .collect()
    }

    fn result(body: &str) -> BatchOperationResult {
        serde_json::from_str(body).unwrap()
    }

    #[test]
    fn a_body_covering_the_batch_is_balanced() {
        let outcome = reconcile(
            &result(r#"{"success":["urn:ngsi-ld:A:1","urn:ngsi-ld:A:2"],"errors":[{"entityId":"urn:ngsi-ld:A:3","error":{}}]}"#),
            &entities(&["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2", "urn:ngsi-ld:A:3"]),
        );

        assert_eq!(outcome.written(), 2);
        assert_eq!(outcome.failed(), 1);
        assert!(outcome.unaccounted().is_empty());
    }

    #[test]
    fn an_under_reporting_body_names_the_entities_it_never_mentioned() {
        let outcome = reconcile(
            &result(r#"{"success":["urn:ngsi-ld:A:1"],"errors":[]}"#),
            &entities(&["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2", "urn:ngsi-ld:A:3"]),
        );

        assert_eq!(outcome.written(), 1);
        assert_eq!(outcome.failed(), 2);
        assert_eq!(
            outcome.unaccounted().iter().map(Iri::as_str).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:A:2", "urn:ngsi-ld:A:3"]
        );
    }

    #[test]
    fn an_over_reporting_body_is_clamped_to_what_was_sent() {
        let outcome = reconcile(
            &result(r#"{"success":["urn:ngsi-ld:A:1","urn:ngsi-ld:A:2","urn:ngsi-ld:A:3"],"errors":[]}"#),
            &entities(&["urn:ngsi-ld:A:1"]),
        );

        assert_eq!(outcome.written(), 1);
        assert_eq!(outcome.failed(), 0);
    }

    #[test]
    fn an_identifier_echoed_in_an_equivalent_form_still_reconciles() {
        // A scheme is case-insensitive under RFC 3986 section 6.2.2.1, the normalization clause 5.5.1
        // adopts, so a broker echoing `URN:` names the same entity Cassiopeia sent as `urn:`.
        let outcome = reconcile(
            &result(r#"{"success":["urn:ngsi-ld:A:1"],"errors":[{"entityId":"URN:ngsi-ld:A:2","error":{}}]}"#),
            &entities(&["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2", "urn:ngsi-ld:A:3"]),
        );

        assert_eq!(outcome.unaccounted().iter().map(Iri::as_str).collect::<Vec<_>>(), vec!["urn:ngsi-ld:A:3"]);
    }
}
