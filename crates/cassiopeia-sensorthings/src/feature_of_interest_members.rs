/// The member an Observation carries its `FeatureOfInterest` in when a read expands it (OGC 18-088
/// section 9.3.2.1).
pub const FEATURE_OF_INTEREST_MEMBER: &str = "FeatureOfInterest";

/// The member a `FeatureOfInterest` holds its feature in, encoded as its `encodingType` says (OGC
/// 18-088 section 8.2.8).
pub const FEATURE_MEMBER: &str = "feature";

/// The `FeatureOfInterest` members a reading's `featureOfInterest` carries as served, besides its id
/// and its feature (OGC 18-088 section 8.2.8, Table 20).
///
/// `properties` is defined from version 1.1 on, so a read of a version 1.1 server selects them with
/// the id, the `encodingType` and the feature, and a read of a version 1.0 server expands the
/// `FeatureOfInterest` without a selection.
pub const CARRIED_FEATURE_OF_INTEREST_MEMBERS: [&str; 3] = ["name", "description", "properties"];

#[cfg(test)]
mod tests {
    use crate::{
        encoding_type::ENCODING_TYPE_MEMBER,
        feature_of_interest_members::{CARRIED_FEATURE_OF_INTEREST_MEMBERS, FEATURE_MEMBER},
    };
    use pretty_assertions::assert_eq;
    use std::collections::BTreeSet;

    #[test]
    fn the_carried_members_are_the_feature_of_interest_properties_besides_its_feature_and_encoding() {
        let properties: BTreeSet<&str> = ["name", "description", "encodingType", "feature", "properties"].into();

        let mut named: BTreeSet<&str> = CARRIED_FEATURE_OF_INTEREST_MEMBERS.into();
        named.insert(FEATURE_MEMBER);
        named.insert(ENCODING_TYPE_MEMBER);

        assert_eq!(named, properties);
        assert_eq!(CARRIED_FEATURE_OF_INTEREST_MEMBERS.len() + 2, properties.len());
    }
}
