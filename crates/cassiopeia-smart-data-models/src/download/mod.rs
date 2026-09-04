pub mod progress;
pub mod published_list;
pub mod replacement;
pub mod schema_kind;
pub mod shared_schema;
pub mod task;

use crate::{
    download::{
        progress::ProgressEvent,
        published_list::{extra_schema_tasks, model_tasks},
        replacement::{Replacement, apply, replacements},
        schema_kind::SchemaKind,
        shared_schema::SHARED_SCHEMAS,
        task::DownloadTask,
    },
    error::{Result, SdmError},
    schema_id::{SchemaId, SchemaName},
    store::{ModelCatalog, SchemaStore},
};
use futures::stream::{self, StreamExt};
use mediatype::{
    MediaTypeBuf,
    names::{APPLICATION, JSON, PLAIN, TEXT},
};
use reqwest::{Client, Response, header::CONTENT_TYPE};
use serde_json::Value;
use std::{collections::BTreeMap, num::NonZeroUsize, str::FromStr, time::Duration};
use tokio::{sync::mpsc::UnboundedSender, time::sleep};
use url::Url;

/// How many schemas are fetched at once when the caller does not say.
const DEFAULT_CONCURRENCY: NonZeroUsize = NonZeroUsize::MIN.saturating_add(7);
/// How many attempts each schema gets when the caller does not say.
const DEFAULT_ATTEMPTS: NonZeroUsize = NonZeroUsize::MIN.saturating_add(2);

/// Whether a served content type marks a document the catalog will accept as a schema.
fn is_schema_media_type(media_type: &MediaTypeBuf) -> bool {
    (media_type.ty() == APPLICATION && media_type.subty() == JSON) || (media_type.ty() == TEXT && media_type.subty() == PLAIN)
}

/// Fetches the published Smart Data Models catalog into a store.
pub struct Downloader {
    concurrency: NonZeroUsize,
    attempts: NonZeroUsize,
    initial_backoff: Duration,
    timeout: Duration,
    progress: Option<UnboundedSender<ProgressEvent>>,
}

impl Default for Downloader {
    fn default() -> Downloader {
        Downloader {
            concurrency: DEFAULT_CONCURRENCY,
            attempts: DEFAULT_ATTEMPTS,
            initial_backoff: Duration::from_millis(500),
            timeout: Duration::from_secs(30),
            progress: None,
        }
    }
}

impl Downloader {
    /// Starts a downloader with the built-in pacing.
    #[must_use]
    pub fn new() -> Downloader {
        Downloader::default()
    }

    /// Sets how many schemas are fetched at once.
    #[must_use]
    pub const fn with_concurrency(mut self, concurrency: NonZeroUsize) -> Downloader {
        self.concurrency = concurrency;
        self
    }

    /// Sets how many attempts each schema gets, and how long to wait before the second one.
    #[must_use]
    pub const fn with_retries(mut self, attempts: NonZeroUsize, initial_backoff: Duration) -> Downloader {
        self.attempts = attempts;
        self.initial_backoff = initial_backoff;
        self
    }

    /// Sets how long a single request may take.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Downloader {
        self.timeout = timeout;
        self
    }

    /// Reports progress over `progress`.
    #[must_use]
    pub fn reporting_to(mut self, progress: UnboundedSender<ProgressEvent>) -> Downloader {
        self.progress = Some(progress);
        self
    }

    /// Fetches every published schema into `store` and records what was stored.
    ///
    /// # Errors
    /// Returns an [`SdmError`] when the HTTP client cannot be built or a published list cannot be
    /// read; individual schema failures are reported and skipped rather than failing the run.
    pub async fn run<S>(&self, store: &S) -> Result<()>
    where
        S: SchemaStore + ModelCatalog,
    {
        let client = Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|source| SdmError::BuildClient { source })?;

        self.report(ProgressEvent::ReadingPublishedLists);
        let tasks = self.plan(&client).await?;
        self.report(ProgressEvent::Planned { schemas: tasks.len() });

        let replacements = replacements(&tasks);
        let stored = self.fetch_all(&client, store, &tasks, &replacements).await;

        store.record_models(&entity_model_ids(&stored))?;
        store.record_context_urls(&context_urls(&tasks))?;

        self.report(ProgressEvent::Finished {
            stored: stored.len(),
            failed: tasks.len() - stored.len(),
        });

        Ok(())
    }

    /// Works out everything that has to be fetched.
    async fn plan(&self, client: &Client) -> Result<Vec<DownloadTask>> {
        let shared = SHARED_SCHEMAS.iter().filter_map(|schema| {
            let id = SchemaId::shared(SchemaName::new(schema.name).ok()?);

            Some(DownloadTask::new(id, schema.url.to_url().ok()?, SchemaKind::Support))
        });

        let mut tasks: Vec<DownloadTask> = shared.collect();
        tasks.extend(model_tasks(client).await?);
        tasks.extend(extra_schema_tasks(client).await?);

        Ok(tasks)
    }

    /// Fetches and stores every task, returning the identifier and kind of each that was stored.
    async fn fetch_all<S>(&self, client: &Client, store: &S, tasks: &[DownloadTask], replacements: &[Replacement]) -> Vec<(SchemaId, SchemaKind)>
    where
        S: SchemaStore + ModelCatalog,
    {
        stream::iter(tasks)
            .map(|task| async move {
                match self.fetch(client, task, replacements).await.and_then(|content| {
                    store.put_schema(task.id(), &content)?;
                    Ok(())
                }) {
                    Ok(()) => {
                        self.report(ProgressEvent::Stored { id: task.id().clone() });
                        Some((task.id().clone(), task.kind()))
                    }
                    Err(error) => {
                        self.report(ProgressEvent::Failed {
                            id: task.id().clone(),
                            reason: error.to_string(),
                        });
                        None
                    }
                }
            })
            .buffer_unordered(self.concurrency.get())
            .filter_map(|stored| async move { stored })
            .collect()
            .await
    }

    /// Fetches one schema, retrying with a doubling backoff.
    async fn fetch(&self, client: &Client, task: &DownloadTask, replacements: &[Replacement]) -> Result<String> {
        let mut backoff = self.initial_backoff;
        let mut last = None;

        for attempt in 0..self.attempts.get() {
            if attempt > 0 {
                sleep(backoff).await;
                backoff *= 2;
            }

            match self.fetch_once(client, task, replacements).await {
                Ok(content) => return Ok(content),
                Err(error) => last = Some(error),
            }
        }

        Err(last.unwrap_or(SdmError::SchemaNotFound { id: task.id().clone() }))
    }

    /// One attempt at fetching a schema, rewritten and re-identified ready to store.
    async fn fetch_once(&self, client: &Client, task: &DownloadTask, replacements: &[Replacement]) -> Result<String> {
        let response = client
            .get(task.url().clone())
            .send()
            .await
            .and_then(Response::error_for_status)
            .map_err(|source| SdmError::Request {
                url: task.url().to_string(),
                source,
            })?;

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();

        // The decision uses a parsed media type (so header parameters are ignored), but the error
        // keeps the server's raw header text for diagnosis.
        let accepted = MediaTypeBuf::from_str(&content_type).is_ok_and(|media_type| is_schema_media_type(&media_type));
        if !accepted {
            return Err(SdmError::UnexpectedContentType {
                url: task.url().to_string(),
                content_type,
            });
        }

        let body = response.text().await.map_err(|source| SdmError::Request {
            url: task.url().to_string(),
            source,
        })?;

        rewrite(&body, task, replacements)
    }

    /// Sends an event, if anybody is listening.
    fn report(&self, event: ProgressEvent) {
        if let Some(progress) = self.progress.as_ref() {
            // A closed channel means the reporter has gone away, which is not the download's
            // problem.
            let _ = progress.send(event);
        }
    }
}

/// Rewrites a fetched schema's references and its `$id` to match where it is being stored.
fn rewrite(body: &str, task: &DownloadTask, replacements: &[Replacement]) -> Result<String> {
    let rewritten = apply(body, replacements);
    let mut schema: Value = serde_json::from_str(&rewritten).map_err(|source| SdmError::DownloadedJson {
        url: task.url().to_string(),
        source,
    })?;

    if let Some(id @ Value::String(_)) = schema.get_mut("$id") {
        *id = Value::String(format!("{}.json", task.id()));
    }

    serde_json::to_string_pretty(&schema).map_err(|source| SdmError::DownloadedJson {
        url: task.url().to_string(),
        source,
    })
}

/// The `@context` document published for each subject repository being downloaded.
fn context_urls(tasks: &[DownloadTask]) -> BTreeMap<SchemaId, Url> {
    tasks
        .iter()
        .filter_map(|task| {
            let repository = task.id().repository()?;
            let url = Url::parse(&format!(
                "https://raw.githubusercontent.com/smart-data-models/{repository}/master/context.jsonld"
            ))
            .ok()?;

            Some((task.id().clone(), url))
        })
        .collect()
}

/// The identifiers of the entity models among the stored schemas; support schemas are stored on
/// disk but never listed as data models.
fn entity_model_ids(stored: &[(SchemaId, SchemaKind)]) -> Vec<SchemaId> {
    stored
        .iter()
        .filter_map(|(id, kind)| match kind {
            SchemaKind::EntityModel => Some(id.clone()),
            SchemaKind::Support => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        download::{Downloader, context_urls, entity_model_ids, replacement::replacements, rewrite, schema_kind::SchemaKind, task::DownloadTask},
        schema_id::SchemaId,
    };
    use std::str::FromStr;
    use url::Url;

    fn id(value: &str) -> SchemaId {
        SchemaId::from_str(value).expect("the identifier is well formed")
    }

    fn task(id: &str) -> DownloadTask {
        DownloadTask::new(
            SchemaId::from_str(id).expect("the identifier is well formed"),
            Url::parse("https://example.org/schema.json").expect("the URL is well formed"),
            SchemaKind::Support,
        )
    }

    #[test]
    fn rewriting_replaces_the_schema_id_with_where_it_is_stored() {
        let rewritten = rewrite(r#"{"$id": "https://example.org/original.json"}"#, &task("dataModel.OCF/Sensor"), &[]).unwrap();

        assert!(rewritten.contains(r#""dataModel.OCF/Sensor.json""#));
    }

    #[test]
    fn rewriting_leaves_a_schema_without_an_id_alone() {
        let rewritten = rewrite(r#"{"type": "object"}"#, &task("Point"), &[]).unwrap();

        assert!(!rewritten.contains("$id"));
    }

    #[test]
    fn rewriting_rewrites_references_to_other_downloaded_schemas() {
        let tasks = vec![task("dataModel.OCF/Sensor")];
        let rewritten = rewrite(
            r#"{"$ref": "https://smart-data-models.github.io/dataModel.OCF/Sensor/schema.json"}"#,
            &task("dataModel.OCF/Device"),
            &replacements(&tasks),
        )
        .unwrap();

        assert!(rewritten.contains(r#""dataModel.OCF/Sensor.json""#));
    }

    #[test]
    fn a_body_that_is_not_json_is_rejected() {
        assert!(rewrite("<html></html>", &task("Point"), &[]).is_err());
    }

    #[test]
    fn a_context_url_is_derived_for_every_repository_qualified_schema() {
        let urls = context_urls(&[task("dataModel.OCF/Sensor"), task("Point")]);

        assert_eq!(urls.len(), 1);
        assert_eq!(
            urls.values().next().map(Url::as_str),
            Some("https://raw.githubusercontent.com/smart-data-models/dataModel.OCF/master/context.jsonld")
        );
    }

    #[test]
    fn only_entity_models_are_listed_among_the_stored_schemas() {
        let stored = vec![
            (id("MultiPoint"), SchemaKind::Support),
            (id("dataModel.Weather/WeatherObserved"), SchemaKind::EntityModel),
            (id("dataModel.Weather/weather-schema"), SchemaKind::Support),
        ];

        assert_eq!(entity_model_ids(&stored), vec![id("dataModel.Weather/WeatherObserved")]);
    }

    #[test]
    fn the_built_in_pacing_fetches_several_schemas_at_once_and_retries() {
        let downloader = Downloader::new();

        assert_eq!(downloader.concurrency.get(), 8);
        assert_eq!(downloader.attempts.get(), 3);
    }
}
