use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// How a batch upsert reconciles an entity that already exists in the broker.
///
/// ETSI GS CIM 009 v1.9.1 clause 5.6.8: batch upsert replaces existing entities by default;
/// `?options=update` switches it to updating attributes in place instead of replacing the whole
/// entity. This enum picks between those two behaviours and rides on
/// [`BrokerOperation::Upsert`](crate::broker_operation::BrokerOperation::Upsert).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum UpsertMode {
    /// Replace each existing entity wholesale (clause 5.6.8 default behaviour).
    #[default]
    Replace,
    /// Update the attributes of each existing entity in place (`?options=update`).
    Update,
}

#[cfg(test)]
mod tests {
    use crate::upsert_mode::UpsertMode;

    #[test]
    fn the_default_mode_is_replace() {
        assert_eq!(UpsertMode::default(), UpsertMode::Replace);
    }

    #[test]
    fn every_mode_round_trips_through_its_kebab_case_token() {
        for (mode, token) in [(UpsertMode::Replace, r#""replace""#), (UpsertMode::Update, r#""update""#)] {
            assert_eq!(serde_json::to_string(&mode).unwrap(), token);
            assert_eq!(serde_json::from_str::<UpsertMode>(token).unwrap(), mode);
        }
    }
}
