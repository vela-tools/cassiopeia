use crate::{input_config::InputConfig, local_context_error::LocalContextError};
use cassiopeia_common::context::mode::AtContextMode;
use cassiopeia_diagnostic::{
    code::{context_code::ContextCode, diagnostic_code::DiagnosticCode},
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_expander::router::MappingRouter;
use cassiopeia_mapping::mapping::Mapping;
use cassiopeia_ngsi_ld::entity::context::{ContextSource, NgsiLdContext};
use cassiopeia_reporter::reporter::DiagnosticSink;
use cassiopeia_smart_data_models::{
    catalog::{Caching, Catalog},
    schema_id::SchemaId,
    store::file_system::FileSystemStore,
};
use std::{collections::HashMap, error::Error, fs, path::Path};

/// Resolves the `@context` to attach to a run's entities from its loaded mappings.
///
/// A single mapping overall, or a non-`Default` global mode, yields one [`ContextSource::Static`];
/// several mappings under `Default` (whether across inputs or across the collections of one
/// multi-collection input), or any per-input override, yield a [`ContextSource::PerEntityType`] map
/// keyed by entity type. A catalog lookup that fails is reported and skipped: the catalog may simply
/// not publish a context for the model.
///
/// # Errors
///
/// Returns [`LocalContextError`] when a local `@context` file the run asked for cannot be read or
/// is not a usable JSON-LD context document. Writing without it would change what the data means.
pub(crate) fn resolve_context_source(
    loaded: &[(&InputConfig, MappingRouter)],
    global_mode: &AtContextMode,
    schemas_folder: &Path,
    sink: &dyn DiagnosticSink,
) -> Result<ContextSource, LocalContextError> {
    let has_per_input = loaded.iter().any(|(input, _)| input.context_mode.is_some());
    let total_mappings: usize = loaded.iter().map(|(_, router)| router.mappings().len()).sum();
    let needs_per_input = has_per_input || (matches!(global_mode, AtContextMode::Default) && total_mappings > 1);

    if !needs_per_input {
        return match loaded.first().and_then(|(_, router)| router.mappings().into_iter().next()) {
            Some(mapping) => {
                let mapping: &Mapping = mapping;
                Ok(match resolve_for_mode(global_mode, mapping, schemas_folder, sink)? {
                    Some(context) => ContextSource::Static(context),
                    None => ContextSource::None,
                })
            }
            None => Ok(ContextSource::None),
        };
    }

    let mut map = HashMap::new();
    for (input, router) in loaded {
        let mode = input.context_mode.as_ref().unwrap_or(global_mode);
        for mapping in router.mappings() {
            let mapping: &Mapping = mapping;
            if let Some(context) = resolve_for_mode(mode, mapping, schemas_folder, sink)? {
                map.insert(mapping.data_model().entity_type().clone(), context);
            }
        }
    }

    Ok(if map.is_empty() {
        ContextSource::None
    } else {
        ContextSource::PerEntityType(map)
    })
}

/// Resolves one input's `@context` according to its mode.
fn resolve_for_mode(
    mode: &AtContextMode,
    mapping: &Mapping,
    schemas_folder: &Path,
    sink: &dyn DiagnosticSink,
) -> Result<Option<NgsiLdContext>, LocalContextError> {
    match mode {
        AtContextMode::None => Ok(None),
        AtContextMode::Explicit(url) => Ok(Some(NgsiLdContext::remote(url.clone()))),
        AtContextMode::Default => Ok(resolve_from_catalog(mapping, schemas_folder, sink)),
        AtContextMode::Local(path) => resolve_local(path).map(Some),
    }
}

/// Reports one degradation of the run's `@context`, which is never fatal: an entity without a
/// context is still an entity.
fn degraded(sink: &dyn DiagnosticSink, code: ContextCode, headline: String, error: &dyn Error) {
    sink.report(
        &DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Context(code), headline)
            .because(error)
            .build(),
    );
}

/// Looks up the context URL a mapping's data model publishes in the Smart Data Models catalog.
fn resolve_from_catalog(mapping: &Mapping, schemas_folder: &Path, sink: &dyn DiagnosticSink) -> Option<NgsiLdContext> {
    let data_model = mapping.data_model();
    let id = match SchemaId::try_from(data_model) {
        Ok(id) => id,
        Err(error) => {
            degraded(
                sink,
                ContextCode::ModelIdInvalid,
                format!("Cannot resolve the @context: '{data_model}' is not a valid model id"),
                &error,
            );
            return None;
        }
    };

    let store = match FileSystemStore::new(schemas_folder) {
        Ok(store) => store,
        Err(error) => {
            degraded(
                sink,
                ContextCode::StoreUnavailable,
                "Cannot resolve the @context: the schema store is unavailable".to_owned(),
                &error,
            );
            return None;
        }
    };

    let catalog = Catalog::new(store, Caching::Enabled);
    match catalog.get_context_url(&id) {
        Ok(Some(url)) => Some(NgsiLdContext::remote(url)),
        Ok(None) => None,
        Err(error) => {
            degraded(
                sink,
                ContextCode::LookupFailed,
                format!("Cannot look up the @context for '{data_model}'; continuing without it"),
                &error,
            );
            None
        }
    }
}

/// Reads a local `.jsonld` file and extracts its `@context` value.
fn resolve_local(path: &Path) -> Result<NgsiLdContext, LocalContextError> {
    let text = fs::read_to_string(path).map_err(|source| LocalContextError::Unreadable {
        path: path.to_path_buf(),
        source,
    })?;
    NgsiLdContext::from_document(&text).map_err(|source| LocalContextError::Unusable {
        path: path.to_path_buf(),
        source,
    })
}
