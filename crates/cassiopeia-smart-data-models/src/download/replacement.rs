use crate::download::task::DownloadTask;
use derive_more::{Deref, Display};

/// The stored path a `$ref` is rewritten to point at (for example `dataModel.OCF/Sensor.json`).
#[derive(Debug, Clone, PartialEq, Eq, Deref, Display)]
pub struct StoredSchemaRef(String);

impl StoredSchemaRef {
    /// Wraps a stored-schema path.
    #[must_use]
    pub fn new(value: impl Into<String>) -> StoredSchemaRef {
        StoredSchemaRef(value.into())
    }
}

/// Rewrites of published URLs into the paths the store keeps those schemas at.
///
/// A downloaded schema's `$ref`s point at URLs on `smart-data-models.github.io` or
/// `raw.githubusercontent.com`. Validation has to work offline, so every reference is rewritten to
/// the stored copy before the schema is written. The same document is published under both hosts,
/// which is why most schemas appear twice here.
const PUBLISHED_URLS: &[(&str, &str)] = &[
    ("https://smart-data-models.github.io/data-models/common-schema.json", "common-schema.json"),
    (
        "https://github.com/smart-data-models/data-models/raw/master/common-schema.json",
        "common-schema.json",
    ),
    (
        "https://raw.githubusercontent.com/smart-data-models/data-models/master/common-schema.json",
        "common-schema.json",
    ),
    ("https://smart-data-models.github.io/dataModel.Hl7/hl7-schema.json", "hl7-schema.json"),
    (
        "https://smart-data-models.github.io/dataModel.Environment/Environment-schema.json",
        "Environment-schema.json",
    ),
    (
        "https://raw.githubusercontent.com/smart-data-models/dataModel.Environment/master/Environment-schema.json",
        "Environment-schema.json",
    ),
    (
        "https://smart-data-models.github.io/dataModel.Weather/Weather-schema.json",
        "Weather-schema.json",
    ),
    (
        "https://smart-data-models.github.io/dataModel.VerifiableCredentials/VerifiableCredentials-schema.json",
        "VerifiableCredentials-schema.json",
    ),
    (
        "https://raw.githubusercontent.com/smart-data-models/dataModel.VerifiableCredentials/master/VerifiableCredentials-schema.json",
        "VerifiableCredentials-schema.json",
    ),
    ("https://smart-data-models.github.io/dataModel.S4BLDG/S4BLDG-schema.json", "S4BLDG-schema.json"),
    (
        "https://raw.githubusercontent.com/smart-data-models/dataModel.S4BLDG/master/S4BLDG-schema.json",
        "S4BLDG-schema.json",
    ),
    (
        "https://smart-data-models.github.io/dataModel.AutonomousMobileRobot/AutonomousMobileRobot-schema.json",
        "AutonomousMobileRobot-schema.json",
    ),
    (
        "https://raw.githubusercontent.com/smart-data-models/dataModel.AutonomousMobileRobot/master/AutonomousMobileRobot-schema.json",
        "AutonomousMobileRobot-schema.json",
    ),
    (
        "https://smart-data-models.github.io/dataModel.HumanResources/HumanResources-schema.json",
        "HumanResources-schema.json",
    ),
    (
        "https://raw.githubusercontent.com/smart-data-models/dataModel.HumanResources/master/HumanResources-schema.json",
        "HumanResources-schema.json",
    ),
    (
        "https://smart-data-models.github.io/dataModel.WaterDistributionManagementEPANET/WaterNetworkManagement-schema.json",
        "WaterNetworkManagement-schema.json",
    ),
    (
        "https://raw.githubusercontent.com/smart-data-models/dataModel.WaterDistributionManagementEPANET/master/WaterNetworkManagement-schema.json",
        "WaterNetworkManagement-schema.json",
    ),
    (
        "https://smart-data-models.github.io/incubated/refs/heads/smartmanufacturing-processindustry/SMARTMANUFACTURING/ProcessIndustry/processindustry-schema.json",
        "processindustry-schema.json",
    ),
    (
        "https://raw.githubusercontent.com/smart-data-models/incubated/refs/heads/smartmanufacturing-processindustry/SMARTMANUFACTURING/ProcessIndustry/processindustry-schema.json",
        "processindustry-schema.json",
    ),
    (
        "https://smart-data-models.github.io/dataModel.Multimedia/Multimedia-schema.json",
        "Multimedia-schema.json",
    ),
    (
        "https://raw.githubusercontent.com/smart-data-models/dataModel.Multimedia/master/Multimedia-schema.json",
        "Multimedia-schema.json",
    ),
    ("https://smart-data-models.github.io/dataModel.SAREF/SAREF-schema.json", "SAREF-schema.json"),
    (
        "https://raw.githubusercontent.com/smart-data-models/dataModel.SAREF/master/SAREF-schema.json",
        "SAREF-schema.json",
    ),
    (
        "https://smart-data-models.github.io/dataModel.DataSpace/DataSpace-schema.json",
        "DataSpace-schema.json",
    ),
    (
        "https://raw.githubusercontent.com/smart-data-models/dataModel.DataSpace/master/DataSpace-schema.json",
        "DataSpace-schema.json",
    ),
    ("https://geojson.org/schema/Point.json", "Point.json"),
    ("http://geojson.org/schema/Point.json", "Point.json"),
    ("https://geojson.org/schema/MultiPoint.json", "MultiPoint.json"),
    ("http://geojson.org/schema/MultiPoint.json", "MultiPoint.json"),
    ("https://geojson.org/schema/Polygon.json", "Polygon.json"),
    ("http://geojson.org/schema/Polygon.json", "Polygon.json"),
    ("https://geojson.org/schema/MultiPolygon.json", "MultiPolygon.json"),
    ("http://geojson.org/schema/MultiPolygon.json", "MultiPolygon.json"),
    (
        "https://smart-data-models.github.io/dataModel.WasteWater/WasteWater-schema.json",
        "WasteWater-schema.json",
    ),
    (
        "https://raw.githubusercontent.com/smart-data-models/dataModel.WasteWater/main/WasteWater-schema.json",
        "WasteWater-schema.json",
    ),
];

/// One published URL and the stored path that replaces it.
#[derive(Debug, PartialEq, Eq)]
pub struct Replacement {
    /// The exact published-URL text to find. A literal string, not a [`url::Url`]: it is applied by
    /// byte-for-byte substitution against downloaded schema text, and URL normalization would change
    /// the bytes and break matching.
    published: String,
    /// The stored path the published URL is rewritten to.
    stored: StoredSchemaRef,
}

/// Every rewrite applied to a downloaded schema before it is stored.
///
/// The fixed table covers the shared schemas; the rest is derived from what is actually being
/// downloaded, so a model added upstream needs no change here.
#[must_use]
pub fn replacements(tasks: &[DownloadTask]) -> Vec<Replacement> {
    let fixed = PUBLISHED_URLS.iter().map(|(published, stored)| Replacement {
        published: (*published).to_string(),
        stored: StoredSchemaRef::new(*stored),
    });

    let derived = tasks.iter().flat_map(|task| {
        let Some(repository) = task.id().repository() else {
            return Vec::new();
        };
        let stored = format!("{}.json", task.id());
        let name = task.id().name();

        vec![
            Replacement {
                published: format!("https://smart-data-models.github.io/{repository}/{name}/schema.json"),
                stored: StoredSchemaRef::new(stored.clone()),
            },
            Replacement {
                published: format!("https://raw.githubusercontent.com/smart-data-models/{repository}/master/{name}/schema.json"),
                stored: StoredSchemaRef::new(stored),
            },
        ]
    });

    fixed.chain(derived).collect()
}

/// Applies every rewrite to a downloaded schema.
#[must_use]
pub fn apply(content: &str, replacements: &[Replacement]) -> String {
    replacements.iter().fold(content.to_string(), |content, replacement| {
        content.replace(&replacement.published, replacement.stored.as_str())
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        download::{
            replacement::{PUBLISHED_URLS, Replacement, StoredSchemaRef, apply, replacements},
            schema_kind::SchemaKind,
            task::DownloadTask,
        },
        schema_id::SchemaId,
    };
    use std::str::FromStr;
    use url::Url;

    fn task(id: &str, url: &str) -> DownloadTask {
        DownloadTask::new(
            SchemaId::from_str(id).expect("the identifier is well formed"),
            Url::parse(url).expect("the URL is well formed"),
            SchemaKind::EntityModel,
        )
    }

    #[test]
    fn every_published_url_in_the_fixed_table_is_a_url() {
        for (published, _) in PUBLISHED_URLS {
            assert!(Url::parse(published).is_ok(), "{published} is not a URL");
        }
    }

    #[test]
    fn a_reference_to_a_published_model_is_rewritten_to_its_stored_path() {
        let tasks = vec![task("dataModel.OCF/Sensor", "https://example.org/schema.json")];
        let rewritten = apply(
            r#"{"$ref": "https://smart-data-models.github.io/dataModel.OCF/Sensor/schema.json"}"#,
            &replacements(&tasks),
        );

        assert_eq!(rewritten, r#"{"$ref": "dataModel.OCF/Sensor.json"}"#);
    }

    #[test]
    fn a_reference_to_the_common_schema_is_rewritten_whichever_host_published_it() {
        let rewritten = apply(
            r#"["https://smart-data-models.github.io/data-models/common-schema.json","https://raw.githubusercontent.com/smart-data-models/data-models/master/common-schema.json"]"#,
            &replacements(&[]),
        );

        assert_eq!(rewritten, r#"["common-schema.json","common-schema.json"]"#);
    }

    #[test]
    fn a_shared_schema_contributes_no_derived_rewrite() {
        let derived = replacements(&[task("common-schema", "https://example.org/common-schema.json")]);

        assert_eq!(derived.len(), PUBLISHED_URLS.len());
    }

    #[test]
    fn content_naming_nothing_published_is_left_alone() {
        let content = r#"{"type": "object"}"#;

        assert_eq!(apply(content, &replacements(&[])), content);
    }

    #[test]
    fn a_replacement_is_compared_by_both_of_its_halves() {
        let first = Replacement {
            published: "a".to_string(),
            stored: StoredSchemaRef::new("b"),
        };
        let second = Replacement {
            published: "a".to_string(),
            stored: StoredSchemaRef::new("c"),
        };

        assert_ne!(first, second);
    }
}
