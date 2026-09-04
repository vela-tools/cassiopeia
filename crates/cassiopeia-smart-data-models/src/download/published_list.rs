use crate::{
    download::{schema_kind::SchemaKind, task::DownloadTask},
    error::{Result, SdmError},
    schema_id::{SchemaId, SchemaName},
};
use reqwest::Client;
use serde::{Deserialize, de::DeserializeOwned};

/// Where the official list of entity models is published.
const MODEL_LIST_URL: &str =
    "https://raw.githubusercontent.com/smart-data-models/data-models/refs/heads/master/specs/AllSubjects/official_list_data_models.json";
/// Where the list of extra per-subject schemas is published.
const EXTRA_SCHEMA_LIST_URL: &str = "https://raw.githubusercontent.com/smart-data-models/data-models/refs/heads/master/specs/AllSubjects/extra_schemas.json";

/// One subject repository and the entity models it publishes, as the official list writes them.
#[derive(Debug, Deserialize)]
struct ModelListEntry {
    #[serde(rename = "repoName")]
    repository: String,

    #[serde(rename = "dataModels")]
    models: Vec<String>,
}

/// One extra schema, as the extra-schema list writes it.
#[derive(Debug, Deserialize)]
struct ExtraSchemaEntry {
    #[serde(rename = "repoName")]
    repository: String,

    #[serde(rename = "schemaName")]
    schema: String,
}

/// The envelope both published lists share.
#[derive(Debug, Deserialize)]
struct PublishedList<T> {
    #[serde(rename = "officialList")]
    entries: Vec<T>,
}

/// Fetches the official entity models and turns each into a download task.
///
/// An entry naming something the store cannot hold is skipped rather than failing the whole
/// download: the list is published upstream and one malformed name should not cost the catalog.
///
/// # Errors
/// Returns an [`SdmError`] when the list cannot be fetched or parsed.
pub async fn model_tasks(client: &Client) -> Result<Vec<DownloadTask>> {
    let list: PublishedList<ModelListEntry> = fetch(client, MODEL_LIST_URL).await?;

    Ok(model_download_tasks(list))
}

/// Fetches the extra per-subject schemas and turns each into a download task.
///
/// # Errors
/// Returns an [`SdmError`] when the list cannot be fetched or parsed.
pub async fn extra_schema_tasks(client: &Client) -> Result<Vec<DownloadTask>> {
    let list: PublishedList<ExtraSchemaEntry> = fetch(client, EXTRA_SCHEMA_LIST_URL).await?;

    Ok(extra_schema_download_tasks(list))
}

/// Turns the official model list into download tasks, skipping entries the store cannot hold.
fn model_download_tasks(list: PublishedList<ModelListEntry>) -> Vec<DownloadTask> {
    list.entries
        .into_iter()
        .flat_map(|entry| {
            entry.models.into_iter().filter_map(move |model| {
                let id = SchemaId::in_repository(SchemaName::new(&entry.repository).ok()?, SchemaName::new(&model).ok()?);
                let url = format!(
                    "https://raw.githubusercontent.com/smart-data-models/{}/master/{model}/schema.json",
                    entry.repository
                );

                Some(DownloadTask::new(id, url.parse().ok()?, SchemaKind::EntityModel))
            })
        })
        .collect()
}

/// Turns the extra-schema list into download tasks, skipping entries the store cannot hold.
fn extra_schema_download_tasks(list: PublishedList<ExtraSchemaEntry>) -> Vec<DownloadTask> {
    list.entries
        .into_iter()
        .filter_map(|entry| {
            let stem = entry.schema.strip_suffix(".json").unwrap_or(&entry.schema);
            let id = SchemaId::in_repository(SchemaName::new(&entry.repository).ok()?, SchemaName::new(stem).ok()?);
            let url = format!(
                "https://raw.githubusercontent.com/smart-data-models/{}/master/{}",
                entry.repository, entry.schema
            );

            Some(DownloadTask::new(id, url.parse().ok()?, SchemaKind::Support))
        })
        .collect()
}

/// Reads one published list.
async fn fetch<T>(client: &Client, url: &str) -> Result<PublishedList<T>>
where
    T: DeserializeOwned,
{
    client
        .get(url)
        .send()
        .await
        .map_err(|source| SdmError::Request { url: url.to_string(), source })?
        .json()
        .await
        .map_err(|source| SdmError::Request { url: url.to_string(), source })
}

#[cfg(test)]
mod tests {
    use crate::download::{
        published_list::{ExtraSchemaEntry, ModelListEntry, PublishedList, extra_schema_download_tasks, model_download_tasks},
        schema_kind::SchemaKind,
    };

    #[test]
    fn a_model_entry_becomes_one_task_per_model_under_its_repository() {
        let list = PublishedList {
            entries: vec![ModelListEntry {
                repository: "dataModel.OCF".to_string(),
                models: vec!["Sensor".to_string(), "Device".to_string()],
            }],
        };

        let tasks = model_download_tasks(list);

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id().to_string(), "dataModel.OCF/Sensor");
        assert_eq!(tasks[1].id().to_string(), "dataModel.OCF/Device");
        assert!(tasks[0].url().as_str().ends_with("dataModel.OCF/master/Sensor/schema.json"));
        assert_eq!(tasks[0].kind(), SchemaKind::EntityModel);
    }

    #[test]
    fn a_model_the_store_cannot_name_is_skipped_rather_than_failing_the_list() {
        let list = PublishedList {
            entries: vec![ModelListEntry {
                repository: "dataModel.OCF".to_string(),
                models: vec!["Sensor".to_string(), "../escape".to_string()],
            }],
        };

        let tasks = model_download_tasks(list);

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id().to_string(), "dataModel.OCF/Sensor");
    }

    #[test]
    fn an_extra_schema_entry_drops_its_json_suffix_from_the_stored_name() {
        let list = PublishedList {
            entries: vec![ExtraSchemaEntry {
                repository: "dataModel.Weather".to_string(),
                schema: "weather-schema.json".to_string(),
            }],
        };

        let tasks = extra_schema_download_tasks(list);

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id().to_string(), "dataModel.Weather/weather-schema");
        assert!(tasks[0].url().as_str().ends_with("dataModel.Weather/master/weather-schema.json"));
        assert_eq!(tasks[0].kind(), SchemaKind::Support);
    }
}
