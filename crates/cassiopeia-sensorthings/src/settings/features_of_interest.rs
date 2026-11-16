use serde::{Deserialize, Serialize};

/// Whether a read requests every Observation with its `FeatureOfInterest`: the `featuresOfInterest`
/// member of a `sensorthings` block.
///
/// The member is a JSON boolean on the wire, `true` for [`FeaturesOfInterest::Requested`] and
/// `false` for [`FeaturesOfInterest::Omitted`]. A `FeatureOfInterest` is where an Observation was
/// made (OGC 18-088 section 8.2.8), which for a Thing that moves is a place of its own for every
/// reading. Expanding it asks a server for one more join per Observation, so a read asks for it only
/// when told to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(from = "bool", into = "bool")]
pub enum FeaturesOfInterest {
    /// Every Observation is requested with its `FeatureOfInterest` (`featuresOfInterest: true`).
    Requested,
    /// Every Observation is requested without it (`featuresOfInterest: false`, or no member).
    #[default]
    Omitted,
}

// Written by hand: no derive maps a `bool` onto the variants of a two-variant enum.
impl From<bool> for FeaturesOfInterest {
    fn from(requested: bool) -> FeaturesOfInterest {
        if requested {
            FeaturesOfInterest::Requested
        } else {
            FeaturesOfInterest::Omitted
        }
    }
}

// Written by hand: no derive maps the variants of a two-variant enum onto a `bool`.
impl From<FeaturesOfInterest> for bool {
    fn from(features: FeaturesOfInterest) -> bool {
        match features {
            FeaturesOfInterest::Requested => true,
            FeaturesOfInterest::Omitted => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::features_of_interest::FeaturesOfInterest;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn a_json_boolean_reads_as_requested_or_omitted() {
        assert_eq!(
            serde_json::from_value::<FeaturesOfInterest>(json!(true)).unwrap(),
            FeaturesOfInterest::Requested
        );
        assert_eq!(serde_json::from_value::<FeaturesOfInterest>(json!(false)).unwrap(), FeaturesOfInterest::Omitted);
    }

    #[test]
    fn the_setting_is_written_back_as_a_json_boolean() {
        assert_eq!(serde_json::to_value(FeaturesOfInterest::Requested).unwrap(), json!(true));
        assert_eq!(serde_json::to_value(FeaturesOfInterest::Omitted).unwrap(), json!(false));
    }

    #[test]
    fn anything_but_a_boolean_is_refused() {
        for value in [json!("true"), json!(1), json!(null)] {
            assert!(serde_json::from_value::<FeaturesOfInterest>(value.clone()).is_err(), "{value}");
        }
    }

    #[test]
    fn features_of_interest_are_omitted_by_default() {
        assert_eq!(FeaturesOfInterest::default(), FeaturesOfInterest::Omitted);
    }
}
