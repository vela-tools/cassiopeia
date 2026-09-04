use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Whether a batch update overwrites attributes that already exist on the target entity.
///
/// ETSI GS CIM 009 v1.9.1 clause 5.6.9: batch update overwrites existing attributes by default;
/// `?options=noOverwrite` preserves any attribute already present and only adds the ones missing.
/// This enum picks between those two behaviours and rides on
/// [`BrokerOperation::Update`](crate::broker_operation::BrokerOperation::Update).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum AttributeOverwrite {
    /// Overwrite existing attributes with the incoming values (clause 5.6.9 default behaviour).
    #[default]
    Overwrite,
    /// Preserve existing attributes and add only the missing ones (`?options=noOverwrite`).
    NoOverwrite,
}

#[cfg(test)]
mod tests {
    use crate::attribute_overwrite::AttributeOverwrite;

    #[test]
    fn the_default_is_overwrite() {
        assert_eq!(AttributeOverwrite::default(), AttributeOverwrite::Overwrite);
    }

    #[test]
    fn every_variant_round_trips_through_its_kebab_case_token() {
        for (overwrite, token) in [
            (AttributeOverwrite::Overwrite, r#""overwrite""#),
            (AttributeOverwrite::NoOverwrite, r#""no-overwrite""#),
        ] {
            assert_eq!(serde_json::to_string(&overwrite).unwrap(), token);
            assert_eq!(serde_json::from_str::<AttributeOverwrite>(token).unwrap(), overwrite);
        }
    }
}
