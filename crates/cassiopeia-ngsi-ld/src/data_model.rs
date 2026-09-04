use crate::entity::{
    error::{NgsiLdError, Result},
    name::NameBuf,
};
use derive_more::Display;
use lazy_regex::regex_is_match;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError};
use std::{result::Result as StdResult, str::FromStr};

/// The repository half of a qualified Smart Data Model identifier, such as `dataModel.OCF`.
///
/// Smart Data Models are published per subject repository; the repository qualifier is what
/// disambiguates two models that share a name across subjects.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
#[display("{_0}")]
pub struct DataModelRepository(String);

impl DataModelRepository {
    /// Builds a repository qualifier, rejecting anything that is not a dot-separated
    /// sequence of alphanumeric segments.
    ///
    /// # Errors
    /// Returns [`NgsiLdError::InvalidDataModelRepository`] when `value` is malformed.
    pub fn new(value: impl Into<String>) -> Result<DataModelRepository> {
        let value = value.into();
        if regex_is_match!(r"^[A-Za-z][A-Za-z0-9]*(?:\.[A-Za-z][A-Za-z0-9]*)*$", &value) {
            Ok(DataModelRepository(value))
        } else {
            Err(NgsiLdError::InvalidDataModelRepository { rejected: value.into() })
        }
    }

    /// The qualifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A Smart Data Model identifier, optionally qualified by the repository that publishes it.
///
/// Written in a mapping as either `Sensor` or `dataModel.OCF/Sensor`. The unqualified form
/// resolves against the whole catalog, so it is only unambiguous when a single repository
/// publishes that model name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
pub enum DataModel {
    /// A model named together with its publishing repository.
    #[display("{repository}/{name}")]
    Qualified {
        /// The publishing repository.
        repository: DataModelRepository,
        /// The model name, which is also the NGSI-LD entity type.
        name: NameBuf,
    },

    /// A model named on its own, to be resolved against the whole catalog.
    #[display("{name}")]
    Unqualified {
        /// The model name, which is also the NGSI-LD entity type.
        name: NameBuf,
    },
}

impl DataModel {
    /// The NGSI-LD entity type this model maps to, which is the model name without its
    /// repository qualifier.
    #[must_use]
    pub const fn entity_type(&self) -> &NameBuf {
        match self {
            DataModel::Qualified { name, .. } | DataModel::Unqualified { name } => name,
        }
    }

    /// The publishing repository, when the identifier carries one.
    #[must_use]
    pub const fn repository(&self) -> Option<&DataModelRepository> {
        match self {
            DataModel::Qualified { repository, .. } => Some(repository),
            DataModel::Unqualified { .. } => None,
        }
    }
}

// Manual because the split on `/` decides which variant is produced, which no derive expresses.
impl FromStr for DataModel {
    type Err = NgsiLdError;

    fn from_str(value: &str) -> StdResult<DataModel, Self::Err> {
        match value.split_once('/') {
            Some((repository, name)) => Ok(DataModel::Qualified {
                repository: DataModelRepository::new(repository)?,
                name: NameBuf::new(name)?,
            }),
            None => Ok(DataModel::Unqualified { name: NameBuf::new(value)? }),
        }
    }
}

// Manual because the wire form is the single `Display` string, not the enum's variant structure.
impl Serialize for DataModel {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

// Manual for the same reason as `Serialize`: the wire form is a single string.
impl<'de> Deserialize<'de> for DataModel {
    fn deserialize<D>(deserializer: D) -> StdResult<DataModel, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        DataModel::from_str(&value).map_err(DeError::custom)
    }
}

#[cfg(test)]
mod tests {
    use crate::data_model::{DataModel, DataModelRepository};
    use std::str::FromStr;

    #[test]
    fn qualified_identifier_splits_into_repository_and_name() {
        let model = DataModel::from_str("dataModel.OCF/Sensor").unwrap();

        assert_eq!(
            model,
            DataModel::Qualified {
                repository: DataModelRepository::new("dataModel.OCF").unwrap(),
                name: "Sensor".parse().unwrap(),
            }
        );
        assert_eq!(model.entity_type().as_str(), "Sensor");
    }

    #[test]
    fn unqualified_identifier_keeps_the_whole_string_as_the_entity_type() {
        let model = DataModel::from_str("AirQualityObserved").unwrap();

        assert_eq!(model.entity_type().as_str(), "AirQualityObserved");
        assert_eq!(model.repository(), None);
    }

    #[test]
    fn display_round_trips_through_from_str() {
        for input in ["dataModel.OCF/Sensor", "AirQualityObserved"] {
            assert_eq!(DataModel::from_str(input).unwrap().to_string(), input);
        }
    }

    #[test]
    fn repository_qualifier_rejects_a_path_separator() {
        assert!(DataModelRepository::new("dataModel.OCF/Sensor").is_err());
    }

    #[test]
    fn model_name_that_is_not_a_valid_ngsi_ld_name_is_rejected() {
        assert!(DataModel::from_str("dataModel.OCF/9Sensor").is_err());
    }

    #[test]
    fn deserializes_from_a_plain_json_string() {
        let model: DataModel = serde_json::from_str(r#""dataModel.Weather/WeatherObserved""#).unwrap();

        assert_eq!(model.entity_type().as_str(), "WeatherObserved");
        assert_eq!(serde_json::to_string(&model).unwrap(), r#""dataModel.Weather/WeatherObserved""#);
    }
}
