use crate::{
    carried_members::{CarriedMembers, remove_carried_units},
    error::{Result, ValidatorError},
    report::ValidationReportEntry,
    required_members::RequiredMembers,
    retriever::FileRetriever,
    schema_file::load_schema,
    schema_location::SchemaLocation,
    schema_validator_config::SchemaValidatorConfig,
    schema_verdict::{DiagnosticsLevel, ValidationDiagnostics, ValidationOutcome},
    schema_violations::SchemaViolations,
    validator::Validator,
};
use cassiopeia_common::representation::NgsiLdRepresentation;
use cassiopeia_ngsi_ld::{
    data_model::DataModelRepository,
    entity::{NgsiLdEntity, name::NameBuf, representation::NgsiLdSerializable},
};
use dashmap::DashMap;
use foldhash::fast::RandomState;
use jsonschema::Validator as JsonSchemaValidator;
use rayon::prelude::*;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// A JSON Schema validator for NGSI-LD entities.
///
/// Each entity is serialized to JSON and checked against `<schemas_folder>/<EntityType>.json`. An
/// entity type with no schema file yields
/// [`SchemaVerdict::Absent`](crate::schema_verdict::SchemaVerdict::Absent), so validation is opt-in
/// per type. Compiled schemas are cached in a lock-free [`DashMap`], so
/// [`check`](Validator::check) is safe to call from many threads at once. Both caches are keyed by
/// an entity type the mapping declared, so they hash with `foldhash` rather than the standard
/// library's `SipHash`: the keys are trusted configuration, and every entity probes them.
#[derive(Clone)]
pub struct SchemaValidator {
    /// Compiled validators, cached by entity type on first use.
    validators: Arc<DashMap<NameBuf, Arc<JsonSchemaValidator>, RandomState>>,
    /// Entity types found to have no schema file, so their absence is not re-checked on disk.
    missing_schemas: Arc<DashMap<NameBuf, (), RandomState>>,
    /// Directory holding the JSON Schema files.
    schemas_folder: PathBuf,
    /// The publishing repository for each entity type, used to resolve a repo-nested schema file.
    repositories: HashMap<NameBuf, DataModelRepository>,
    /// The explicit schema file for each entity type that requested a custom one.
    custom_schemas: HashMap<NameBuf, PathBuf>,
    /// The NGSI-LD representation entities are serialized in before validation.
    representation: NgsiLdRepresentation,
}

impl SchemaValidator {
    /// Builds a validator from its configuration.
    #[must_use]
    pub fn new(config: SchemaValidatorConfig) -> SchemaValidator {
        SchemaValidator {
            validators: Arc::new(DashMap::with_hasher(RandomState::default())),
            missing_schemas: Arc::new(DashMap::with_hasher(RandomState::default())),
            schemas_folder: config.schemas_folder,
            repositories: config.repositories,
            custom_schemas: config.custom_schemas,
            representation: config.representation,
        }
    }

    /// Where an entity type's schema is found: an explicit custom file if one was requested for it,
    /// otherwise the Smart Data Models convention path.
    fn schema_location(&self, entity_type: &NameBuf) -> SchemaLocation {
        match self.custom_schemas.get(entity_type) {
            Some(path) => SchemaLocation::Explicit(path.clone()),
            None => SchemaLocation::Convention(self.convention_path(entity_type)),
        }
    }

    /// The convention schema file path for an entity type.
    ///
    /// A type whose mapping named a qualified data model resolves to its repository subdirectory
    /// (`<folder>/<repository>/<Type>.json`), the layout `sdm download` writes; every other type
    /// resolves to a file at the folder root.
    fn convention_path(&self, entity_type: &NameBuf) -> PathBuf {
        match self.repositories.get(entity_type) {
            Some(repository) => self.schemas_folder.join(repository.as_str()).join(format!("{entity_type}.json")),
            None => self.schemas_folder.join(format!("{entity_type}.json")),
        }
    }

    /// The ordered `$ref` resolution roots for an explicit custom schema: its own directory first, so
    /// a schema can reference a sibling definition file, then the shared schemas folder for common or
    /// Smart Data Model references.
    fn explicit_roots(&self, schema_path: &Path) -> Vec<PathBuf> {
        let mut roots = Vec::with_capacity(2);
        if let Some(parent) = schema_path.parent() {
            roots.push(parent.to_path_buf());
        }
        roots.push(self.schemas_folder.clone());
        roots
    }

    /// Returns the cached validator for an entity type, compiling its schema on first use.
    ///
    /// Yields `Ok(None)` when the entity type has no convention schema file: that type is recorded as
    /// missing so the absence is not re-checked on disk. An explicitly requested schema that is
    /// absent is a [`ValidatorError::CustomSchemaMissing`] rather than a tolerated absence. A
    /// present-but-broken schema (unreadable, unparseable, null, or uncompilable) is an error every
    /// time rather than a silent skip.
    fn get_or_load_validator(&self, entity_type: &NameBuf) -> Result<Option<Arc<JsonSchemaValidator>>> {
        if let Some(validator) = self.validators.get(entity_type) {
            return Ok(Some(Arc::clone(&validator)));
        }
        if self.missing_schemas.contains_key(entity_type) {
            return Ok(None);
        }

        let (schema_path, roots) = match self.schema_location(entity_type) {
            SchemaLocation::Explicit(path) => {
                if !path.exists() {
                    return Err(ValidatorError::CustomSchemaMissing {
                        // The error owns its context after this borrowed key is released.
                        entity_type: entity_type.clone(),
                        path,
                    });
                }
                let roots = self.explicit_roots(&path);
                (path, roots)
            }
            SchemaLocation::Convention(path) => {
                if !path.exists() {
                    // The cache key must own the type name; cloning it once per unseen type is unavoidable.
                    self.missing_schemas.insert(entity_type.clone(), ());
                    return Ok(None);
                }
                (path, vec![self.schemas_folder.clone()])
            }
        };

        let schema = load_schema(&schema_path)?;
        let validator = jsonschema::options()
            // Each compiled validator owns a retriever rooted at the paths this schema resolves against.
            .with_retriever(FileRetriever::rooted(roots))
            .with_base_uri("http://localhost/")
            .build(&schema)
            .map_err(|source| ValidatorError::CompileValidator {
                source: Box::new(source),
                // The returned error owns its context after this borrowed key is released.
                entity_type: entity_type.clone(),
            })?;

        let validator = Arc::new(validator);
        // The cache key must own the type name; cloning it once per compiled schema is unavoidable.
        self.validators.insert(entity_type.clone(), Arc::clone(&validator));

        Ok(Some(validator))
    }

    /// Checks one entity against an already-resolved schema, or reports it as unvalidated when the
    /// type has none.
    ///
    /// Taking the schema as an argument is what lets a batch resolve once per distinct entity type
    /// instead of once per entity: the compiled validator is immutable once built and every entity of
    /// a run names the same handful of types, so the per-entity probe was contending on one shard for
    /// an answer that never changes. Borrowing it rather than taking the `Arc` also spares a refcount
    /// bump per entity.
    ///
    /// An entity whose required members are carried, and that fails its schema, has every violation
    /// read to tell the members it lacks from the rest, whatever diagnostics were asked for: the
    /// verdict depends on whether anything else is broken.
    fn check_against(
        &self,
        entity: &NgsiLdEntity,
        schema: Option<&JsonSchemaValidator>,
        diagnostics_level: DiagnosticsLevel,
        required: RequiredMembers,
    ) -> Result<ValidationOutcome> {
        let Some(validator) = schema else {
            return Ok(ValidationOutcome::absent());
        };

        let entity_value = entity.to_json(self.representation).map_err(|source| ValidatorError::SerializeEntity {
            source,
            entity_type: entity.entity_type.clone(),
        })?;

        if validator.is_valid(&entity_value) {
            return Ok(ValidationOutcome::conformant());
        }

        let failure = Failure {
            validator,
            entity_value: &entity_value,
            entity,
        };
        match required {
            RequiredMembers::Decided => {
                let diagnostics = failure.diagnostics(diagnostics_level, required, || SchemaViolations::collect(validator, &entity_value))?;
                Ok(ValidationOutcome::nonconformant(diagnostics))
            }
            RequiredMembers::Carried => {
                let (carried, violations) = CarriedMembers::split(validator, &entity_value);
                if violations.is_empty() {
                    return Ok(ValidationOutcome::conformant().carrying(carried));
                }
                let diagnostics = failure.diagnostics(diagnostics_level, required, || violations)?;
                Ok(ValidationOutcome::nonconformant(diagnostics).carrying(carried))
            }
        }
    }

    /// Resolves every distinct entity type in a batch to its compiled schema, once per type.
    ///
    /// A type whose resolution errors is omitted: [`ValidatorError`] is not `Clone`, so the failure
    /// cannot be fanned out from the table and those entities fall back to the per-entity path, which
    /// produces the same error for each of them.
    fn schemas_for_batch(&self, entities: &[NgsiLdEntity]) -> Vec<(NameBuf, Option<Arc<JsonSchemaValidator>>)> {
        let mut resolved: Vec<(NameBuf, Option<Arc<JsonSchemaValidator>>)> = Vec::new();
        for entity in entities {
            if resolved.iter().any(|(known, _)| *known == entity.entity_type) {
                continue;
            }
            // The type name must be owned: the table outlives the borrow of the entity it came from.
            if let Ok(schema) = self.get_or_load_validator(&entity.entity_type) {
                resolved.push((entity.entity_type.clone(), schema));
            }
        }
        resolved
    }
}

impl Validator for SchemaValidator {
    fn check(&self, entity: &NgsiLdEntity, diagnostics_level: DiagnosticsLevel, required: RequiredMembers) -> Result<ValidationOutcome> {
        self.check_against(entity, self.get_or_load_validator(&entity.entity_type)?.as_deref(), diagnostics_level, required)
    }

    fn validate_batch(&self, entities: &[NgsiLdEntity], diagnostics: DiagnosticsLevel, required: RequiredMembers) -> Vec<Result<ValidationOutcome>> {
        let resolved = self.schemas_for_batch(entities);
        entities
            .par_iter()
            .map(|entity| match resolved.iter().find(|(known, _)| *known == entity.entity_type) {
                Some((_, schema)) => self.check_against(entity, schema.as_deref(), diagnostics, required),
                // A type absent from the table failed to resolve; the per-entity path reports that
                // failure for each of its entities unchanged.
                None => self.check(entity, diagnostics, required),
            })
            .collect()
    }
}

/// One entity known to fail its schema, and what it failed.
struct Failure<'a> {
    /// The schema it failed.
    validator: &'a JsonSchemaValidator,
    /// The entity, serialized as it was checked.
    entity_value: &'a Value,
    /// The entity itself.
    entity: &'a NgsiLdEntity,
}

impl Failure<'_> {
    /// The diagnostics `level` asks for, naming `violations`, the rules the entity broke that are
    /// decided on it, gathered only when they are asked for.
    fn diagnostics(&self, level: DiagnosticsLevel, required: RequiredMembers, violations: impl FnOnce() -> SchemaViolations) -> Result<ValidationDiagnostics> {
        match level {
            DiagnosticsLevel::None => Ok(ValidationDiagnostics::None),
            DiagnosticsLevel::Errors => Ok(ValidationDiagnostics::Errors {
                error: Box::new(self.validation_error(violations())),
            }),
            DiagnosticsLevel::Report => {
                let error = Box::new(self.validation_error(violations()));
                let entry = ValidationReportEntry {
                    // Report entries outlive the borrowed entity and therefore own their identity.
                    entity_id: self.entity.id.clone(),
                    entity_type: self.entity.entity_type.clone(),
                    evaluation: self.evaluation(required)?,
                };
                Ok(ValidationDiagnostics::Report { error, entry })
            }
        }
    }

    /// The entity's list-format evaluation, without the units of members carried rather than decided.
    fn evaluation(&self, required: RequiredMembers) -> Result<Value> {
        let mut evaluation = serde_json::to_value(self.validator.evaluate(self.entity_value).list()).map_err(|source| ValidatorError::SerializeEvaluation {
            source,
            // The error must own its context after this borrowed entity leaves the check.
            entity_type: self.entity.entity_type.clone(),
        })?;
        match required {
            RequiredMembers::Decided => {}
            RequiredMembers::Carried => remove_carried_units(&mut evaluation),
        }
        Ok(evaluation)
    }

    /// The failure naming `violations`.
    fn validation_error(&self, violations: SchemaViolations) -> ValidatorError {
        ValidatorError::ValidationFailed {
            // Diagnostics must own their identity after the borrowed entity leaves validation.
            entity_id: self.entity.id.clone(),
            entity_type: self.entity.entity_type.clone(),
            violations,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::ValidatorError,
        required_members::RequiredMembers,
        schema_validator::SchemaValidator,
        schema_validator_config::SchemaValidatorConfig,
        schema_verdict::{DiagnosticsLevel, SchemaVerdict, ValidationDiagnostics, ValidationOutcome},
        validator::Validator,
    };
    use cassiopeia_common::representation::NgsiLdRepresentation;
    use cassiopeia_ngsi_ld::{
        data_model::DataModelRepository,
        entity::{
            NgsiLdEntity,
            attribute::{NgsiLdAttribute, NgsiLdAttributeWrapper, property::NgsiLdProperty},
            name::NameBuf,
        },
        value::types::Value,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::{collections::HashMap, fs, path::PathBuf};
    use tempfile::TempDir;
    use urn_rs::Urn;

    fn validator(folder: PathBuf) -> SchemaValidator {
        validator_with_repositories(folder, HashMap::new())
    }

    fn validator_with_repositories(folder: PathBuf, repositories: HashMap<NameBuf, DataModelRepository>) -> SchemaValidator {
        SchemaValidator::new(SchemaValidatorConfig {
            schemas_folder: folder,
            repositories,
            custom_schemas: HashMap::new(),
            representation: NgsiLdRepresentation::Normalized,
        })
    }

    fn validator_with_custom_schemas(folder: PathBuf, custom_schemas: HashMap<NameBuf, PathBuf>) -> SchemaValidator {
        SchemaValidator::new(SchemaValidatorConfig {
            schemas_folder: folder,
            repositories: HashMap::new(),
            custom_schemas,
            representation: NgsiLdRepresentation::Normalized,
        })
    }

    fn entity(entity_type: &str, id: &str) -> NgsiLdEntity {
        NgsiLdEntity::new(id.parse::<Urn>().unwrap(), NameBuf::new(entity_type).unwrap())
    }

    /// A `Sensor` entity whose `pm10` property holds `pm10`.
    fn sensor_reading(pm10: serde_json::Value) -> NgsiLdEntity {
        let mut sensor = entity("Sensor", "urn:ngsi-ld:Sensor:1");
        sensor.attributes.insert(
            NameBuf::new("pm10").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty::new(Value::try_from(pm10).unwrap()))),
        );
        sensor
    }

    /// A validator over a key-values `Sensor` schema that requires `dateObserved` and `location`, and
    /// `pm10` to be a number, in the folder `directory`.
    fn requiring_validator(directory: &TempDir) -> SchemaValidator {
        fs::write(
            directory.path().join("Sensor.json"),
            r#"{"type": "object", "properties": {"pm10": {"type": "number"}}, "required": ["id", "type", "dateObserved", "location"]}"#,
        )
        .unwrap();
        SchemaValidator::new(SchemaValidatorConfig {
            schemas_folder: directory.path().to_path_buf(),
            repositories: HashMap::new(),
            custom_schemas: HashMap::new(),
            representation: NgsiLdRepresentation::Simplified,
        })
    }

    /// The names of the members an outcome carries.
    fn carried(outcome: &mut ValidationOutcome) -> Vec<String> {
        outcome.take_carried().members().map(ToString::to_string).collect()
    }

    #[test]
    fn an_entity_type_without_a_schema_is_absent() {
        let directory = TempDir::new().unwrap();
        let validator = validator(directory.path().to_path_buf());

        let outcome = validator
            .check(&entity("Unknown", "urn:ngsi-ld:Unknown:1"), DiagnosticsLevel::None, RequiredMembers::Decided)
            .unwrap();

        assert_eq!(outcome.verdict(), SchemaVerdict::Absent);
    }

    #[test]
    fn an_entity_meeting_its_schema_is_conformant() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("Sensor.json"), r#"{"type": "object", "required": ["id", "type"]}"#).unwrap();
        let validator = validator(directory.path().to_path_buf());

        let outcome = validator
            .check(&entity("Sensor", "urn:ngsi-ld:Sensor:1"), DiagnosticsLevel::Report, RequiredMembers::Decided)
            .unwrap();

        assert_eq!(outcome.verdict(), SchemaVerdict::Conformant);
        assert!(matches!(outcome.into_diagnostics(), ValidationDiagnostics::None));
    }

    #[test]
    fn a_qualified_type_resolves_its_schema_under_the_repository_subdirectory() {
        // `sdm download` writes catalog schemas at `<folder>/<repository>/<Type>.json`; a mapping
        // that named the qualified data model must resolve the schema there, not at the root.
        let directory = TempDir::new().unwrap();
        fs::create_dir(directory.path().join("dataModel.Transportation")).unwrap();
        fs::write(
            directory.path().join("dataModel.Transportation/BikeHireDockingStation.json"),
            r#"{"type": "object", "required": ["temperature"]}"#,
        )
        .unwrap();
        let repositories = HashMap::from([(
            NameBuf::new("BikeHireDockingStation").unwrap(),
            DataModelRepository::new("dataModel.Transportation").unwrap(),
        )]);
        let validator = validator_with_repositories(directory.path().to_path_buf(), repositories);

        let outcome = validator
            .check(
                &entity("BikeHireDockingStation", "urn:ngsi-ld:BikeHireDockingStation:1"),
                DiagnosticsLevel::None,
                RequiredMembers::Decided,
            )
            .unwrap();

        // The schema was found under the repository subdirectory and its `temperature` requirement
        // was applied, so a bare entity is nonconformant rather than absent.
        assert_eq!(outcome.verdict(), SchemaVerdict::Nonconformant);
        assert!(matches!(outcome.into_diagnostics(), ValidationDiagnostics::None));
    }

    #[test]
    fn an_entity_missing_a_required_property_is_nonconformant() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("Sensor.json"), r#"{"type": "object", "required": ["temperature"]}"#).unwrap();
        let validator = validator(directory.path().to_path_buf());

        let outcome = validator
            .check(&entity("Sensor", "urn:ngsi-ld:Sensor:1"), DiagnosticsLevel::Report, RequiredMembers::Decided)
            .unwrap();

        assert_eq!(outcome.verdict(), SchemaVerdict::Nonconformant);
        let ValidationDiagnostics::Report { error, entry } = outcome.into_diagnostics() else {
            panic!("expected report diagnostics");
        };
        assert!(matches!(*error, ValidatorError::ValidationFailed { .. }));
        assert!(!entry.evaluation.is_null());
    }

    #[test]
    fn error_diagnostics_do_not_construct_a_report_entry() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("Sensor.json"), r#"{"type": "object", "required": ["temperature"]}"#).unwrap();
        let validator = validator(directory.path().to_path_buf());

        let outcome = validator
            .check(&entity("Sensor", "urn:ngsi-ld:Sensor:1"), DiagnosticsLevel::Errors, RequiredMembers::Decided)
            .unwrap();

        assert_eq!(outcome.verdict(), SchemaVerdict::Nonconformant);
        let ValidationDiagnostics::Errors { error } = outcome.into_diagnostics() else {
            panic!("expected error diagnostics without a report entry");
        };
        assert!(matches!(*error, ValidatorError::ValidationFailed { .. }));
    }

    #[test]
    fn a_null_schema_file_is_an_error() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("Sensor.json"), "null").unwrap();
        let validator = validator(directory.path().to_path_buf());

        let result = validator.check(&entity("Sensor", "urn:ngsi-ld:Sensor:1"), DiagnosticsLevel::None, RequiredMembers::Decided);

        assert!(matches!(result, Err(ValidatorError::SchemaFileIsNull { .. })));
    }

    #[test]
    fn an_explicit_local_custom_schema_is_applied() {
        // The schema lives at a path of the run's choosing, not the convention path; an entity is
        // checked against it purely because the type is listed in `custom_schemas`.
        let directory = TempDir::new().unwrap();
        let schema_path = directory.path().join("exoplanet.schema.json");
        fs::write(&schema_path, r#"{"type": "object", "required": ["mass"]}"#).unwrap();
        let custom = HashMap::from([(NameBuf::new("ExoPlanet").unwrap(), schema_path)]);
        let validator = validator_with_custom_schemas(directory.path().to_path_buf(), custom);

        let outcome = validator
            .check(
                &entity("ExoPlanet", "urn:ngsi-ld:ExoPlanet:1"),
                DiagnosticsLevel::None,
                RequiredMembers::Decided,
            )
            .unwrap();

        assert_eq!(outcome.verdict(), SchemaVerdict::Nonconformant);
    }

    #[test]
    fn a_missing_explicit_schema_is_an_error() {
        // A convention-absent schema is `Absent`; an explicitly requested one that is absent aborts.
        let directory = TempDir::new().unwrap();
        let custom = HashMap::from([(NameBuf::new("ExoPlanet").unwrap(), directory.path().join("nonexistent.json"))]);
        let validator = validator_with_custom_schemas(directory.path().to_path_buf(), custom);

        let result = validator.check(
            &entity("ExoPlanet", "urn:ngsi-ld:ExoPlanet:1"),
            DiagnosticsLevel::None,
            RequiredMembers::Decided,
        );

        assert!(matches!(result, Err(ValidatorError::CustomSchemaMissing { .. })));
    }

    #[test]
    fn an_explicit_schema_overrides_the_convention_file() {
        // The convention file at the folder root is lax; the explicit schema is strict. The explicit
        // one is what applies, so a bare entity is nonconformant rather than conformant.
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("ExoPlanet.json"), r#"{"type": "object"}"#).unwrap();
        let strict = directory.path().join("strict.schema.json");
        fs::write(&strict, r#"{"type": "object", "required": ["mass"]}"#).unwrap();
        let custom = HashMap::from([(NameBuf::new("ExoPlanet").unwrap(), strict)]);
        let validator = validator_with_custom_schemas(directory.path().to_path_buf(), custom);

        let outcome = validator
            .check(
                &entity("ExoPlanet", "urn:ngsi-ld:ExoPlanet:1"),
                DiagnosticsLevel::None,
                RequiredMembers::Decided,
            )
            .unwrap();

        assert_eq!(outcome.verdict(), SchemaVerdict::Nonconformant);
    }

    #[test]
    fn an_explicit_schema_resolves_a_sibling_ref() {
        // The schema references `./defs.json`, present only in the schema's own directory and not in
        // the shared schemas folder, so it must resolve via the schema-dir root ahead of the folder.
        let schema_dir = TempDir::new().unwrap();
        let schemas_folder = TempDir::new().unwrap();
        fs::write(
            schema_dir.path().join("exoplanet.schema.json"),
            r#"{"type": "object", "required": ["mass"], "properties": {"mass": {"$ref": "defs.json"}}}"#,
        )
        .unwrap();
        fs::write(schema_dir.path().join("defs.json"), r#"{"type": "number"}"#).unwrap();
        let custom = HashMap::from([(NameBuf::new("ExoPlanet").unwrap(), schema_dir.path().join("exoplanet.schema.json"))]);
        let validator = validator_with_custom_schemas(schemas_folder.path().to_path_buf(), custom);

        // The schema compiled (the sibling `$ref` resolved) and its `mass` requirement was applied.
        let outcome = validator
            .check(
                &entity("ExoPlanet", "urn:ngsi-ld:ExoPlanet:1"),
                DiagnosticsLevel::None,
                RequiredMembers::Decided,
            )
            .unwrap();

        assert_eq!(outcome.verdict(), SchemaVerdict::Nonconformant);
    }

    #[test]
    fn a_batch_checks_every_entity_in_order() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("Sensor.json"), r#"{"type": "object", "required": ["temperature"]}"#).unwrap();
        let validator = validator(directory.path().to_path_buf());
        let entities = vec![entity("Unknown", "urn:ngsi-ld:Unknown:1"), entity("Sensor", "urn:ngsi-ld:Sensor:1")];

        let results = validator.validate_batch(&entities, DiagnosticsLevel::None, RequiredMembers::Decided);

        assert_eq!(results.len(), 2);
        assert!(matches!(&results[0], Ok(outcome) if outcome.verdict() == SchemaVerdict::Absent));
        assert!(matches!(&results[1], Ok(outcome) if outcome.verdict() == SchemaVerdict::Nonconformant));
    }

    #[test]
    fn a_batch_of_one_type_matches_checking_each_entity_on_its_own() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("Sensor.json"), r#"{"type": "object", "required": ["id", "type"]}"#).unwrap();
        let validator = validator(directory.path().to_path_buf());
        let entities = vec![
            entity("Sensor", "urn:ngsi-ld:Sensor:1"),
            entity("Sensor", "urn:ngsi-ld:Sensor:2"),
            entity("Sensor", "urn:ngsi-ld:Sensor:3"),
        ];

        let batched = validator.validate_batch(&entities, DiagnosticsLevel::None, RequiredMembers::Decided);

        // Resolving the schema once for the whole batch must not change any entity's verdict.
        for (index, result) in batched.iter().enumerate() {
            let alone = validator.check(&entities[index], DiagnosticsLevel::None, RequiredMembers::Decided).unwrap();
            assert_eq!(result.as_ref().unwrap().verdict(), alone.verdict());
            assert_eq!(result.as_ref().unwrap().verdict(), SchemaVerdict::Conformant);
        }
    }

    #[test]
    fn a_batch_of_a_type_with_no_schema_is_absent_for_every_entity() {
        let directory = TempDir::new().unwrap();
        let validator = validator(directory.path().to_path_buf());
        let entities = vec![entity("Unknown", "urn:ngsi-ld:Unknown:1"), entity("Unknown", "urn:ngsi-ld:Unknown:2")];

        let results = validator.validate_batch(&entities, DiagnosticsLevel::None, RequiredMembers::Decided);

        assert!(
            results
                .iter()
                .all(|result| matches!(result, Ok(outcome) if outcome.verdict() == SchemaVerdict::Absent))
        );
    }

    #[test]
    fn a_batch_mixing_a_schema_backed_type_with_an_unknown_one_verdicts_each_by_its_own_type() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("Sensor.json"), r#"{"type": "object", "required": ["id", "type"]}"#).unwrap();
        let validator = validator(directory.path().to_path_buf());
        let entities = vec![entity("Sensor", "urn:ngsi-ld:Sensor:1"), entity("Unknown", "urn:ngsi-ld:Unknown:1")];

        let results = validator.validate_batch(&entities, DiagnosticsLevel::None, RequiredMembers::Decided);

        assert_eq!(results.len(), 2);
        assert!(matches!(&results[0], Ok(outcome) if outcome.verdict() == SchemaVerdict::Conformant));
        assert!(matches!(&results[1], Ok(outcome) if outcome.verdict() == SchemaVerdict::Absent));
    }

    #[test]
    fn a_batch_of_a_type_whose_custom_schema_is_missing_errors_for_every_entity_of_that_type() {
        // The type resolves to an error, so it is absent from the batch's schema table and each of its
        // entities must still report the failure through the per-entity path.
        let directory = TempDir::new().unwrap();
        let custom = HashMap::from([(NameBuf::new("Sensor").unwrap(), directory.path().join("absent.schema.json"))]);
        let validator = validator_with_custom_schemas(directory.path().to_path_buf(), custom);
        let entities = vec![entity("Sensor", "urn:ngsi-ld:Sensor:1"), entity("Sensor", "urn:ngsi-ld:Sensor:2")];

        let results = validator.validate_batch(&entities, DiagnosticsLevel::None, RequiredMembers::Decided);

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|result| matches!(result, Err(ValidatorError::CustomSchemaMissing { .. }))));
    }

    #[test]
    fn a_nonconformant_entity_in_a_batch_still_carries_its_diagnostics() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("Sensor.json"), r#"{"type": "object", "required": ["temperature"]}"#).unwrap();
        let validator = validator(directory.path().to_path_buf());
        let entities = vec![entity("Sensor", "urn:ngsi-ld:Sensor:1")];

        let mut results = validator.validate_batch(&entities, DiagnosticsLevel::Errors, RequiredMembers::Decided);

        let outcome = results.remove(0).unwrap();
        assert_eq!(outcome.verdict(), SchemaVerdict::Nonconformant);
        assert!(matches!(outcome.into_diagnostics(), ValidationDiagnostics::Errors { .. }));
    }

    #[test]
    fn an_entity_whose_required_members_are_decided_on_it_violates_its_schema_by_lacking_one() {
        let directory = TempDir::new().unwrap();
        let validator = requiring_validator(&directory);

        let mut outcome = validator
            .check(&sensor_reading(json!(18.2)), DiagnosticsLevel::Errors, RequiredMembers::Decided)
            .unwrap();

        assert_eq!(outcome.verdict(), SchemaVerdict::Nonconformant);
        assert_eq!(carried(&mut outcome), Vec::<String>::new());
    }

    #[test]
    fn an_entity_lacking_only_required_members_that_are_carried_conforms_and_carries_them() {
        let directory = TempDir::new().unwrap();
        let validator = requiring_validator(&directory);

        for level in [DiagnosticsLevel::None, DiagnosticsLevel::Errors, DiagnosticsLevel::Report] {
            let mut outcome = validator.check(&sensor_reading(json!(18.2)), level, RequiredMembers::Carried).unwrap();

            assert_eq!(outcome.verdict(), SchemaVerdict::Conformant, "{level:?}");
            assert_eq!(carried(&mut outcome), ["dateObserved", "location"], "{level:?}");
        }
    }

    #[test]
    fn a_value_of_the_wrong_type_is_still_decided_on_an_entity_whose_required_members_are_carried() {
        let directory = TempDir::new().unwrap();
        let validator = requiring_validator(&directory);

        let mut outcome = validator
            .check(&sensor_reading(json!("high")), DiagnosticsLevel::Report, RequiredMembers::Carried)
            .unwrap();

        assert_eq!(outcome.verdict(), SchemaVerdict::Nonconformant);
        assert_eq!(carried(&mut outcome), ["dateObserved", "location"]);
        let ValidationDiagnostics::Report { error, entry } = outcome.into_diagnostics() else {
            panic!("expected report diagnostics");
        };
        assert_eq!(
            error.to_string(),
            r#"Entity urn:ngsi-ld:Sensor:1 of type 'Sensor' does not conform to its schema: 1 violation(s), first at '/pm10': "high" is not of type "number""#
        );
        let units_with_errors: Vec<&str> = entry.evaluation["details"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|unit| unit.get("errors").is_some())
            .map(|unit| unit["evaluationPath"].as_str().unwrap())
            .collect();
        assert_eq!(units_with_errors, ["/properties/pm10/type"]);
    }

    #[test]
    fn a_batch_carries_the_required_members_of_each_entity() {
        let directory = TempDir::new().unwrap();
        let validator = requiring_validator(&directory);
        let entities = vec![sensor_reading(json!(18.2)), sensor_reading(json!("high"))];

        let mut results = validator.validate_batch(&entities, DiagnosticsLevel::None, RequiredMembers::Carried);

        let mut second = results.remove(1).unwrap();
        let mut first = results.remove(0).unwrap();
        assert_eq!(first.verdict(), SchemaVerdict::Conformant);
        assert_eq!(second.verdict(), SchemaVerdict::Nonconformant);
        assert_eq!(carried(&mut first), ["dateObserved", "location"]);
        assert_eq!(carried(&mut second), ["dateObserved", "location"]);
    }
}
