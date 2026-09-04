use crate::model_picker::PickerEntry;
use cassiopeia_ngsi_ld::data_model::DataModel;
use derive_more::Display;

/// One row of the wizard's model picker: either the "create a new model" action, or an existing
/// catalog model.
///
/// The create action is a distinct variant the picker cannot confuse with a real model.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum ModelChoice {
    /// The action that starts naming a brand-new data model.
    #[display("[+] Create New Data Model")]
    CreateNew,

    /// An existing catalog model the wizard can map against.
    #[display("{_0}")]
    Existing(DataModel),
}

impl ModelChoice {
    /// Whether this row is the "create a new model" action.
    #[must_use]
    pub const fn is_create_new(&self) -> bool {
        matches!(self, ModelChoice::CreateNew)
    }

    /// The existing model this row carries, or `None` for the create action.
    #[must_use]
    pub const fn existing(&self) -> Option<&DataModel> {
        match self {
            ModelChoice::Existing(model) => Some(model),
            ModelChoice::CreateNew => None,
        }
    }
}

impl PickerEntry for ModelChoice {
    fn matches_query(&self, lowercased_query: &str) -> bool {
        match self {
            // The create-new action remains visible in the filtered model list.
            ModelChoice::CreateNew => true,
            ModelChoice::Existing(model) => model.matches_query(lowercased_query),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{model_picker::PickerEntry, screen::wizard::model_choice::ModelChoice};
    use cassiopeia_ngsi_ld::data_model::DataModel;
    use std::str::FromStr;

    fn model(name: &str) -> DataModel {
        DataModel::from_str(name).unwrap()
    }

    #[test]
    fn the_create_action_renders_the_legacy_sentinel_and_always_matches() {
        assert_eq!(ModelChoice::CreateNew.to_string(), "[+] Create New Data Model");
        assert!(ModelChoice::CreateNew.matches_query("anything"));
        assert!(ModelChoice::CreateNew.is_create_new());
    }

    #[test]
    fn an_existing_row_renders_and_matches_its_model() {
        let choice = ModelChoice::Existing(model("Sensor"));
        assert_eq!(choice.to_string(), "Sensor");
        assert!(choice.matches_query("sens"));
        assert!(!choice.matches_query("zzz"));
        assert_eq!(choice.existing(), Some(&model("Sensor")));
    }
}
