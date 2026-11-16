/// The member an Observation holds its result in (OGC 18-088 section 8.2.7).
pub const RESULT_MEMBER: &str = "result";

/// The Observation members a reading carries as served, besides its id, its `result` and its
/// `phenomenonTime` (OGC 18-088 section 8.2.7).
///
/// A range or incremental read selects them with the id, the `phenomenonTime` and the `result`, so
/// a member read from an Observation is one the read asked the server for.
pub const CARRIED_OBSERVATION_MEMBERS: [&str; 4] = ["resultTime", "validTime", "resultQuality", "parameters"];

#[cfg(test)]
mod tests {
    use crate::{
        observation_members::{CARRIED_OBSERVATION_MEMBERS, RESULT_MEMBER},
        phenomenon_time::PHENOMENON_TIME_MEMBER,
    };
    use pretty_assertions::assert_eq;
    use std::collections::BTreeSet;

    #[test]
    fn the_carried_members_are_the_observation_properties_besides_its_result_and_phenomenon_time() {
        let properties: BTreeSet<&str> = ["phenomenonTime", "result", "resultTime", "resultQuality", "validTime", "parameters"].into();

        let mut named: BTreeSet<&str> = CARRIED_OBSERVATION_MEMBERS.into();
        named.insert(RESULT_MEMBER);
        named.insert(PHENOMENON_TIME_MEMBER);

        assert_eq!(named, properties);
        assert_eq!(CARRIED_OBSERVATION_MEMBERS.len() + 2, properties.len());
    }
}
