use crate::{
    error::Result,
    schema_verdict::{DiagnosticsLevel, ValidationOutcome},
};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;

/// Checks NGSI-LD entities for schema compliance.
///
/// Implementations must be thread-safe (`Send + Sync`): a batch of entities may be validated
/// concurrently across many threads.
pub trait Validator: Send + Sync {
    /// Checks a single entity against its schema, returning an absent, conformant, or nonconformant
    /// outcome with the requested level of diagnostics.
    ///
    /// A nonconformance is reported inside the verdict, not as an error: `Ok` covers every case the
    /// schema itself decides.
    ///
    /// # Errors
    /// Returns a [`ValidatorError`](crate::error::ValidatorError) only for an infrastructure failure:
    /// the entity's schema cannot be read, parsed, or compiled, or the entity cannot be serialized.
    fn check(&self, entity: &NgsiLdEntity, diagnostics: DiagnosticsLevel) -> Result<ValidationOutcome>;

    /// Checks a batch of entities, one result per entity in input order.
    ///
    /// The default implementation processes entities sequentially; a failed check never prevents the
    /// rest from being checked.
    ///
    /// # Errors
    /// Each element carries the same result [`check`](Validator::check) would return for that entity;
    /// the batch call itself never fails.
    fn validate_batch(&self, entities: &[NgsiLdEntity], diagnostics: DiagnosticsLevel) -> Vec<Result<ValidationOutcome>> {
        entities.iter().map(|entity| self.check(entity, diagnostics)).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::Result,
        schema_verdict::{DiagnosticsLevel, SchemaVerdict, ValidationOutcome},
        validator::Validator,
    };
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use urn_rs::Urn;

    /// A validator that finds no schema for anything, used to exercise the default batch behaviour.
    struct AlwaysAbsent;

    impl Validator for AlwaysAbsent {
        fn check(&self, _entity: &NgsiLdEntity, _diagnostics: DiagnosticsLevel) -> Result<ValidationOutcome> {
            Ok(ValidationOutcome::absent())
        }
    }

    fn entity(id: &str, entity_type: &str) -> NgsiLdEntity {
        NgsiLdEntity::new(id.parse::<Urn>().unwrap(), NameBuf::new(entity_type).unwrap())
    }

    #[test]
    fn the_default_batch_returns_one_result_per_entity_in_order() {
        let entities = vec![entity("urn:ngsi-ld:Sensor:1", "Sensor"), entity("urn:ngsi-ld:Sensor:2", "Sensor")];

        let results = AlwaysAbsent.validate_batch(&entities, DiagnosticsLevel::None);

        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .all(|result| matches!(result, Ok(outcome) if outcome.verdict() == SchemaVerdict::Absent))
        );
    }
}
