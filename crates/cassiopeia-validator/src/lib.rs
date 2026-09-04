//! Validation stage for the Cassiopeia pipeline.
//!
//! Checks NGSI-LD entities against JSON Schema definitions, one schema per entity type. Schemas are
//! loaded from disk on first use and cached for the lifetime of the validator, and an entity type
//! with no schema file yields [`SchemaVerdict::Absent`](schema_verdict::SchemaVerdict) (validation
//! is opt-in per type). The [`SchemaValidator`](schema_validator::SchemaValidator) is the standard
//! implementation, backed by the [`jsonschema`] crate.

pub mod error;
pub mod report;
pub mod retriever;
pub mod schema_file;
pub mod schema_location;
pub mod schema_validator;
pub mod schema_validator_config;
pub mod schema_verdict;
pub mod schema_violation;
pub mod schema_violation_kind;
pub mod schema_violations;
pub mod validator;

#[cfg(test)]
mod tests {
    use crate::{
        schema_validator::SchemaValidator,
        schema_validator_config::SchemaValidatorConfig,
        schema_verdict::{DiagnosticsLevel, SchemaVerdict},
        validator::Validator,
    };
    use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use std::collections::HashMap;
    use tempfile::TempDir;
    use urn_rs::Urn;

    #[test]
    fn the_public_surface_checks_an_entity_end_to_end() {
        let directory = TempDir::new().unwrap();
        let validator = SchemaValidator::new(SchemaValidatorConfig {
            schemas_folder: directory.path().to_path_buf(),
            repositories: HashMap::new(),
            custom_schemas: HashMap::new(),
            representation: NgsiLdRepresentation::Normalized,
            skip_null: NgsiLdSkipNull::Skip,
        });
        let entity = NgsiLdEntity::new("urn:ngsi-ld:Unknown:1".parse::<Urn>().unwrap(), NameBuf::new("Unknown").unwrap());

        let outcome = validator.check(&entity, DiagnosticsLevel::None).unwrap();
        assert_eq!(outcome.verdict(), SchemaVerdict::Absent);
    }
}
