use cassiopeia_ngsi_ld::{data_model::DataModel, entity::error::NgsiLdError};
use derive_more::Display;
use lazy_regex::regex_is_match;
use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::str::FromStr;
use thiserror::Error;

/// Raised when a schema identifier does not have the shape the catalog can store.
#[derive(Debug, Error)]
pub enum SchemaIdError {
    /// A name held characters outside the storable set, or a `..` segment.
    #[error("'{value}' is not a schema name: expected a letter or digit followed by letters, digits, dots, dashes, or underscores")]
    InvalidName {
        /// The rejected name, echoed back for diagnosis.
        value: String,
    },

    /// A repository name was not a legal NGSI-LD name.
    #[error("'{value}' is not a schema repository")]
    InvalidRepository {
        /// The rejected repository, echoed back for diagnosis.
        value: String,
        /// The validation failure reported by the NGSI-LD layer.
        source: NgsiLdError,
    },
}

/// The name of one schema inside the catalog.
///
/// Wider than an NGSI-LD entity type, because the catalog also holds shared schemas whose names are
/// not entity types at all (`common-schema`, `Point`). Path separators and dot segments are
/// rejected, so a name taken from a remote listing can never escape the store's directory.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Display)]
#[display("{_0}")]
pub struct SchemaName(String);

impl SchemaName {
    /// Builds a schema name, rejecting anything that could name something outside the store.
    ///
    /// # Errors
    /// Returns [`SchemaIdError::InvalidName`] when `value` holds a character outside the storable
    /// set or a `..` segment that could escape the store's directory.
    pub fn new(value: impl Into<String>) -> Result<SchemaName, SchemaIdError> {
        let value = value.into();

        // A leading digit is allowed: Smart Data Models publishes models whose names begin with a
        // digit (`dataModel.OCF/3DPrinter`). The escape guarantee rests on excluding `/` and `..`,
        // not on the first character being a letter.
        if regex_is_match!(r"^[A-Za-z0-9][A-Za-z0-9._-]*$", &value) && !value.contains("..") {
            Ok(SchemaName(value))
        } else {
            Err(SchemaIdError::InvalidName { value })
        }
    }

    /// The name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where one schema lives in the catalog.
///
/// A schema published under a Smart Data Models subject repository carries that repository; the
/// shared schemas every subject refers to carry none.
///
/// The wire form is the single qualified string (`repository/name`, or just `name`), so
/// serialization goes through [`Display`] and deserialization through [`FromStr`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Display, SerializeDisplay, DeserializeFromStr)]
#[display("{}", self.to_qualified_string())]
pub struct SchemaId {
    /// The subject repository the schema was published under, if any.
    repository: Option<SchemaName>,
    /// The schema's own name.
    name: SchemaName,
}

impl SchemaId {
    /// Builds an identifier for a schema published under a subject repository.
    #[must_use]
    pub const fn in_repository(repository: SchemaName, name: SchemaName) -> SchemaId {
        SchemaId {
            repository: Some(repository),
            name,
        }
    }

    /// Builds an identifier for a schema that belongs to no subject repository.
    #[must_use]
    pub const fn shared(name: SchemaName) -> SchemaId {
        SchemaId { repository: None, name }
    }

    /// The subject repository, when the schema belongs to one.
    #[must_use]
    pub const fn repository(&self) -> Option<&SchemaName> {
        self.repository.as_ref()
    }

    /// The schema's own name.
    #[must_use]
    pub const fn name(&self) -> &SchemaName {
        &self.name
    }

    /// The identifier written the way it appears in a manifest and in a `$ref`.
    fn to_qualified_string(&self) -> String {
        match &self.repository {
            Some(repository) => format!("{repository}/{}", self.name),
            None => self.name.to_string(),
        }
    }
}

impl TryFrom<&DataModel> for SchemaId {
    type Error = SchemaIdError;

    fn try_from(model: &DataModel) -> Result<SchemaId, Self::Error> {
        let name = SchemaName::new(model.entity_type().as_str())?;

        match model.repository() {
            Some(repository) => Ok(SchemaId::in_repository(SchemaName::new(repository.as_str())?, name)),
            None => Ok(SchemaId::shared(name)),
        }
    }
}

impl FromStr for SchemaId {
    type Err = SchemaIdError;

    fn from_str(value: &str) -> Result<SchemaId, Self::Err> {
        match value.split_once('/') {
            Some((repository, name)) => Ok(SchemaId::in_repository(SchemaName::new(repository)?, SchemaName::new(name)?)),
            None => Ok(SchemaId::shared(SchemaName::new(value)?)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema_id::{SchemaId, SchemaName};
    use cassiopeia_ngsi_ld::data_model::DataModel;
    use std::str::FromStr;

    #[test]
    fn a_qualified_identifier_keeps_its_repository() {
        let id = SchemaId::from_str("dataModel.OCF/Sensor").unwrap();

        assert_eq!(id.repository().map(SchemaName::as_str), Some("dataModel.OCF"));
        assert_eq!(id.name().as_str(), "Sensor");
        assert_eq!(id.to_string(), "dataModel.OCF/Sensor");
    }

    #[test]
    fn a_shared_schema_has_no_repository() {
        let id = SchemaId::from_str("common-schema").unwrap();

        assert_eq!(id.repository(), None);
        assert_eq!(id.to_string(), "common-schema");
    }

    #[test]
    fn a_name_that_could_escape_the_store_is_rejected() {
        assert!(SchemaName::new("../secrets").is_err());
        assert!(SchemaName::new("..").is_err());
        assert!(SchemaName::new("/etc/passwd").is_err());
        assert!(SchemaId::from_str("dataModel.OCF/../../etc/passwd").is_err());
    }

    #[test]
    fn a_name_may_start_with_a_digit() {
        // `dataModel.OCF/3DPrinter` is a published Smart Data Model; the catalog must load it.
        let id = SchemaId::from_str("dataModel.OCF/3DPrinter").unwrap();

        assert_eq!(id.repository().map(SchemaName::as_str), Some("dataModel.OCF"));
        assert_eq!(id.name().as_str(), "3DPrinter");
        assert_eq!(id.to_string(), "dataModel.OCF/3DPrinter");
    }

    #[test]
    fn a_data_model_converts_to_the_identifier_of_its_schema() {
        let model = DataModel::from_str("dataModel.Weather/WeatherObserved").unwrap();
        let id = SchemaId::try_from(&model).unwrap();

        assert_eq!(id.to_string(), "dataModel.Weather/WeatherObserved");
    }

    #[test]
    fn an_identifier_round_trips_through_json() {
        let id = SchemaId::from_str("dataModel.OCF/Sensor").unwrap();
        let encoded = serde_json::to_string(&id).unwrap();

        assert_eq!(encoded, r#""dataModel.OCF/Sensor""#);
        assert_eq!(serde_json::from_str::<SchemaId>(&encoded).unwrap(), id);
    }
}
