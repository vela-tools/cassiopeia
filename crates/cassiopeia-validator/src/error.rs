use crate::schema_violations::SchemaViolations;
use cassiopeia_common::error::io::IoError;
use cassiopeia_ngsi_ld::entity::{error::NgsiLdError, name::NameBuf};
use jsonschema::ValidationError;
use serde_json::Error as JsonError;
use simd_json::Error as SimdJsonError;
use std::{path::PathBuf, result};
use thiserror::Error;
use urn_rs::Urn;

/// Failures raised while validating an entity against its JSON Schema.
#[derive(Debug, Error)]
pub enum ValidatorError {
    /// A schema file could not be read from disk.
    #[error(transparent)]
    Io(#[from] IoError),

    /// The schema file was empty or contained JSON `null`.
    #[error("Schema file is null or empty: {}", path.display())]
    SchemaFileIsNull {
        /// The offending schema file.
        path: PathBuf,
    },

    /// The schema file could not be parsed as JSON.
    #[error("Failed to parse the schema file at '{}'", path.display())]
    ParseSchemaFile {
        /// The parse failure reported by the JSON reader.
        #[source]
        source: SimdJsonError,
        /// The schema file that could not be parsed.
        path: PathBuf,
    },

    /// The JSON schema could not be compiled into a validator.
    #[error("The schema for entity type '{entity_type}' could not be compiled")]
    CompileValidator {
        /// The compilation error, boxed because `jsonschema::ValidationError` is large enough to
        /// bloat every `Result` in the crate otherwise.
        #[source]
        source: Box<ValidationError<'static>>,
        /// The entity type whose schema failed to compile.
        entity_type: NameBuf,
    },

    /// The entity failed schema validation.
    ///
    /// The message is one line naming the entity and its first violation; the structured list is
    /// carried alongside for anything that wants to report more than a line.
    #[error("Entity {entity_id} of type '{entity_type}' does not conform to its schema: {violations}")]
    ValidationFailed {
        /// The entity that failed validation.
        entity_id: Urn,
        /// Its type.
        entity_type: NameBuf,
        /// How it broke the schema.
        violations: SchemaViolations,
    },

    /// The entity could not be serialized to JSON for validation.
    #[error("An entity of type '{entity_type}' could not be serialized for validation")]
    SerializeEntity {
        /// The serialization failure reported by the NGSI-LD layer.
        #[source]
        source: NgsiLdError,
        /// The entity type that could not be serialized.
        entity_type: NameBuf,
    },

    /// Structured JSON Schema output could not be converted into the report value.
    #[error("The validation report output for entity type '{entity_type}' could not be serialized")]
    SerializeEvaluation {
        /// The serialization failure reported by serde.
        #[source]
        source: JsonError,
        /// The entity type whose evaluation output could not be serialized.
        entity_type: NameBuf,
    },

    /// A caller required validation errors but requested a verdict-only check.
    #[error("Validation diagnostics were not requested for nonconformant entity {entity_id} of type '{entity_type}'")]
    MissingDiagnostics {
        /// The nonconformant entity whose diagnostics were unavailable.
        entity_id: Urn,
        /// Its type.
        entity_type: NameBuf,
    },

    /// An explicitly requested custom schema file was absent. Unlike the opt-in convention, whose
    /// missing schema is tolerated, an explicit source that cannot be loaded aborts the run.
    #[error("Custom schema for entity type '{entity_type}' was not found at {}", path.display())]
    CustomSchemaMissing {
        /// The entity type whose explicit schema was requested.
        entity_type: NameBuf,
        /// The path the schema was expected at.
        path: PathBuf,
    },
}

/// The result type used throughout the validation stage.
pub type Result<T> = result::Result<T, ValidatorError>;

#[cfg(test)]
mod tests {
    use crate::{error::ValidatorError, schema_violations::SchemaViolations};
    use cassiopeia_common::error::io::{IoAction, IoError};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use jsonschema::Validator;
    use serde_json::json;
    use std::{io, path::PathBuf};

    #[test]
    fn an_io_failure_is_wrapped_transparently() {
        let io = IoError::FileOperation {
            source: io::Error::other("boom"),
            path: PathBuf::from("/schemas/Sensor.json"),
            action: IoAction::Read,
        };
        let error = ValidatorError::from(io);

        assert!(error.to_string().contains("Sensor.json"));
    }

    #[test]
    fn a_validation_failure_names_the_entity_the_type_and_its_first_violation() {
        let validator = Validator::new(&json!({"type": "object", "required": ["temperature", "id"]})).unwrap();
        let error = ValidatorError::ValidationFailed {
            entity_id: "urn:ngsi-ld:Sensor:1".parse().unwrap(),
            entity_type: NameBuf::new("Sensor").unwrap(),
            violations: SchemaViolations::collect(&validator, &json!({})),
        };
        let message = error.to_string();

        assert!(message.contains("urn:ngsi-ld:Sensor:1"));
        assert!(message.contains("Sensor"));
        assert!(message.contains('2'));
        assert!(!message.contains('\n'));
    }
}
