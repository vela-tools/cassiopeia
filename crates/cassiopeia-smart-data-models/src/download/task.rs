use crate::{download::schema_kind::SchemaKind, schema_id::SchemaId};
use url::Url;

/// One schema to fetch and where to keep it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTask {
    /// Where the fetched schema is stored in the catalog.
    id: SchemaId,
    /// Where the schema is fetched from.
    url: Url,
    /// Whether the schema is an entity model or a support schema.
    kind: SchemaKind,
}

impl DownloadTask {
    /// Builds a task fetching `url` into the catalog entry `id`.
    #[must_use]
    pub const fn new(id: SchemaId, url: Url, kind: SchemaKind) -> DownloadTask {
        DownloadTask { id, url, kind }
    }

    /// Where the fetched schema is stored.
    #[must_use]
    pub const fn id(&self) -> &SchemaId {
        &self.id
    }

    /// Where the schema is fetched from.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// Whether the schema is an entity model or a support schema.
    #[must_use]
    pub const fn kind(&self) -> SchemaKind {
        self.kind
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        download::{schema_kind::SchemaKind, task::DownloadTask},
        schema_id::SchemaId,
    };
    use std::str::FromStr;
    use url::Url;

    #[test]
    fn a_task_keeps_the_identifier_and_url_it_was_built_with() {
        let id = SchemaId::from_str("dataModel.OCF/Sensor").unwrap();
        let url = Url::parse("https://example.org/schema.json").unwrap();
        let task = DownloadTask::new(id.clone(), url.clone(), SchemaKind::EntityModel);

        assert_eq!(task.id(), &id);
        assert_eq!(task.url(), &url);
        assert_eq!(task.kind(), SchemaKind::EntityModel);
    }
}
