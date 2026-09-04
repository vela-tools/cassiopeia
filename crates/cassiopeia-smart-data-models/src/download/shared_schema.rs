use url::Url;

/// A URL fixed at compile time.
///
/// [`Url`] has no `const` constructor, so a compile-time table of URLs is stored as string slices
/// and parsed to [`Url`] at use. The crate's tests assert every entry parses, so an unparseable
/// literal is caught at test time rather than silently skipped at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticUrl(&'static str);

impl StaticUrl {
    /// Wraps a compile-time URL literal.
    #[must_use]
    pub const fn new(url: &'static str) -> StaticUrl {
        StaticUrl(url)
    }

    /// The URL as a string slice.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// Parses the literal into a [`Url`].
    ///
    /// # Errors
    /// Returns [`url::ParseError`] when the literal is not a valid URL.
    pub fn to_url(self) -> Result<Url, url::ParseError> {
        Url::parse(self.0)
    }
}

/// One schema every subject repository refers to but none of them owns.
///
/// These are fetched by URL rather than derived from the published model list, because the list
/// only names entity models; `common-schema.json`, the per-subject `*-schema.json` documents, and
/// the `GeoJSON` geometry schemas are what those models' `$ref`s point at.
pub struct SharedSchema {
    /// Where the schema is published.
    pub url: StaticUrl,

    /// The name the schema is stored under.
    pub name: &'static str,
}

/// Every schema fetched by URL rather than from the published model list.
pub const SHARED_SCHEMAS: &[SharedSchema] = &[
    SharedSchema {
        url: StaticUrl::new("https://raw.githubusercontent.com/smart-data-models/data-models/refs/heads/master/common-schema.json"),
        name: "common-schema",
    },
    SharedSchema {
        url: StaticUrl::new("https://smart-data-models.github.io/dataModel.Hl7/hl7-schema.json"),
        name: "hl7-schema",
    },
    SharedSchema {
        url: StaticUrl::new("https://raw.githubusercontent.com/smart-data-models/dataModel.Environment/master/Environment-schema.json"),
        name: "Environment-schema",
    },
    SharedSchema {
        url: StaticUrl::new("https://smart-data-models.github.io/dataModel.Weather/weather-schema.json#/definitions/Weather-Commons"),
        name: "Weather-schema",
    },
    SharedSchema {
        url: StaticUrl::new("https://raw.githubusercontent.com/smart-data-models/dataModel.VerifiableCredentials/master/VerifiableCredentials-schema.json"),
        name: "VerifiableCredentials-schema",
    },
    SharedSchema {
        url: StaticUrl::new("https://raw.githubusercontent.com/smart-data-models/dataModel.S4BLDG/master/S4BLDG-schema.json"),
        name: "S4BLDG-schema",
    },
    SharedSchema {
        url: StaticUrl::new("https://raw.githubusercontent.com/smart-data-models/dataModel.AutonomousMobileRobot/master/AutonomousMobileRobot-schema.json"),
        name: "AutonomousMobileRobot-schema",
    },
    SharedSchema {
        url: StaticUrl::new("https://raw.githubusercontent.com/smart-data-models/dataModel.HumanResources/master/HumanResources-schema.json"),
        name: "HumanResources-schema",
    },
    SharedSchema {
        url: StaticUrl::new(
            "https://raw.githubusercontent.com/smart-data-models/dataModel.WaterDistributionManagementEPANET/master/WaterNetworkManagement-schema.json",
        ),
        name: "WaterNetworkManagement-schema",
    },
    SharedSchema {
        url: StaticUrl::new(
            "https://raw.githubusercontent.com/smart-data-models/incubated/refs/heads/smartmanufacturing-processindustry/SMARTMANUFACTURING/ProcessIndustry/processindustry-schema.json",
        ),
        name: "processindustry-schema",
    },
    SharedSchema {
        url: StaticUrl::new("https://raw.githubusercontent.com/smart-data-models/dataModel.Multimedia/master/Multimedia-schema.json"),
        name: "Multimedia-schema",
    },
    SharedSchema {
        url: StaticUrl::new("https://raw.githubusercontent.com/smart-data-models/dataModel.SAREF/master/SAREF-schema.json"),
        name: "SAREF-schema",
    },
    SharedSchema {
        url: StaticUrl::new("https://raw.githubusercontent.com/smart-data-models/dataModel.DataSpace/master/DataSpace-schema.json"),
        name: "DataSpace-schema",
    },
    SharedSchema {
        url: StaticUrl::new("https://geojson.org/schema/Point.json"),
        name: "Point",
    },
    SharedSchema {
        url: StaticUrl::new("https://geojson.org/schema/MultiPoint.json"),
        name: "MultiPoint",
    },
    SharedSchema {
        url: StaticUrl::new("https://geojson.org/schema/Polygon.json"),
        name: "Polygon",
    },
    SharedSchema {
        url: StaticUrl::new("https://geojson.org/schema/MultiPolygon.json"),
        name: "MultiPolygon",
    },
    SharedSchema {
        url: StaticUrl::new("https://raw.githubusercontent.com/smart-data-models/dataModel.WasteWater/main/WasteWater-schema.json"),
        name: "WasteWater-schema",
    },
];

#[cfg(test)]
mod tests {
    use crate::{download::shared_schema::SHARED_SCHEMAS, schema_id::SchemaName};

    #[test]
    fn every_shared_schema_has_a_storable_name() {
        for schema in SHARED_SCHEMAS {
            assert!(SchemaName::new(schema.name).is_ok(), "{} is not a storable name", schema.name);
        }
    }

    #[test]
    fn every_shared_schema_has_a_well_formed_url() {
        for schema in SHARED_SCHEMAS {
            assert!(schema.url.to_url().is_ok(), "{} is not a URL", schema.url.as_str());
        }
    }

    #[test]
    fn no_shared_schema_is_listed_twice() {
        let mut names: Vec<&str> = SHARED_SCHEMAS.iter().map(|schema| schema.name).collect();
        let listed = names.len();
        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), listed);
    }
}
