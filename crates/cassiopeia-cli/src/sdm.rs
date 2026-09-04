use crate::{
    config::CliConfig,
    error::{CliError, Result},
};
use cassiopeia_diagnostic::{
    code::{catalog_code::CatalogCode, diagnostic_code::DiagnosticCode},
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::Reporter;
use cassiopeia_smart_data_models::{
    catalog::{Caching, Catalog},
    download::{Downloader, progress::ProgressEvent},
    store::file_system::FileSystemStore,
};
use std::{num::NonZeroUsize, path::PathBuf};
use tokio::{runtime::Builder, sync::mpsc::unbounded_channel};

/// Downloads the published Smart Data Models catalog into the schemas folder, reporting progress.
///
/// The downloader is asynchronous, so a single-threaded runtime drives it while a spawned task
/// forwards its progress events to the reporter; the reporter must be `'static` because it is shared
/// with that task.
///
/// # Errors
///
/// Returns [`CliError`](crate::error::CliError) when the async runtime cannot be built or the
/// download fails.
pub fn download_schemas(config: &CliConfig, reporter: &'static dyn Reporter) -> Result<PathBuf> {
    let store = FileSystemStore::new(&config.schemas_folder)?;
    let runtime = Builder::new_current_thread().enable_all().build().map_err(CliError::AsyncRuntime)?;

    // Attempts are retries plus the first try; `NonZeroUsize::MIN` is 1, so this is always non-zero.
    let attempts = NonZeroUsize::MIN.saturating_add(config.download.max_retries as usize);
    let downloader = Downloader::new()
        .with_concurrency(config.download.max_concurrent_downloads)
        .with_retries(attempts, config.download.initial_backoff)
        .with_timeout(config.download.http_timeout);

    runtime.block_on(async move {
        let (sender, mut receiver) = unbounded_channel();
        let drain = tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                report_event(reporter, event);
            }
        });

        let result = downloader.reporting_to(sender).run(&store).await;
        let _ = drain.await;

        result
    })?;

    Ok(config.schemas_folder.clone())
}

/// Forwards one download progress event to the reporter.
fn report_event(reporter: &dyn Reporter, event: ProgressEvent) {
    match event {
        ProgressEvent::ReadingPublishedLists => reporter.step(1, 3, "Reading published lists..."),
        ProgressEvent::Planned { schemas } => {
            reporter.step(2, 3, &format!("Downloading {schemas} schemas..."));
            reporter.start_progress("Downloading");
            reporter.progress_set_length(schemas as u64);
        }
        ProgressEvent::Stored { id } => {
            reporter.progress_inc();
            reporter.update_progress(&id.to_string());
        }
        ProgressEvent::Failed { id, reason } => {
            reporter.progress_inc();
            reporter.update_progress(&id.to_string());
            reporter.report(
                &DiagnosticBuilder::new(
                    Severity::Warning,
                    DiagnosticCode::Catalog(CatalogCode::SchemaDownloadFailed),
                    format!("Schema '{id}' could not be downloaded"),
                )
                .with_cause(reason)
                .build(),
            );
        }
        ProgressEvent::Finished { stored, failed } => {
            reporter.stop_progress();
            reporter.step(3, 3, "Finalizing...");
            if failed > 0 {
                reporter.report(
                    &DiagnosticBuilder::new(
                        Severity::Warning,
                        DiagnosticCode::Catalog(CatalogCode::SchemaDownloadIncomplete),
                        format!("Downloaded {stored} schemas; {failed} could not be downloaded"),
                    )
                    .build(),
                );
            } else {
                reporter.success(&format!("Downloaded {stored} schemas successfully"));
            }
        }
    }
}

/// Lists every schema in the stored catalog by name.
///
/// # Errors
///
/// Returns [`CliError`](crate::error::CliError) when the catalog store cannot be read.
pub fn list_schemas(config: &CliConfig) -> Result<Vec<String>> {
    let store = FileSystemStore::new(&config.schemas_folder)?;
    let catalog = Catalog::new(store, Caching::Disabled);
    Ok(catalog.list()?.iter().map(ToString::to_string).collect())
}

/// Lists the stored schemas whose name matches `query`.
///
/// # Errors
///
/// Returns [`CliError`](crate::error::CliError) when the catalog store cannot be read.
pub fn search_schemas(config: &CliConfig, query: &str) -> Result<Vec<String>> {
    let store = FileSystemStore::new(&config.schemas_folder)?;
    let catalog = Catalog::new(store, Caching::Disabled);
    Ok(catalog.search(query)?.iter().map(ToString::to_string).collect())
}
