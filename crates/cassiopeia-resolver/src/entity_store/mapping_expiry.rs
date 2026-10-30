use crate::mapping_id::MappingId;
use ahash::AHashMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The entity `expiresAt` one mapping contributes to a base id, folded from its records.
///
/// `expiresAt` says when the stored entity is to be deleted (ETSI GS CIM 009 v1.9.1 clause 4.22),
/// so how a mapping's records fold into it follows how the store folds the records themselves: a
/// record that supersedes another replaces its expiry with its own, a record merged into another
/// overrides it only when it declares one, and a series keeps whichever of its observations lasts
/// longest. Several mappings contributing to one id are then joined by [`join`].
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MappingExpiry(Option<DateTime<Utc>>);

impl MappingExpiry {
    /// The expiry of a mapping's first record for an id.
    #[must_use]
    pub const fn first(declared: Option<DateTime<Utc>>) -> MappingExpiry {
        MappingExpiry(declared)
    }

    /// Takes the expiry of a record that supersedes the retained one, a record without one leaving
    /// the mapping without one: the superseded record's data is gone, and its expiry with it.
    pub const fn supersede(&mut self, declared: Option<DateTime<Utc>>) {
        self.0 = declared;
    }

    /// Takes the expiry of a record merged into the retained one when it declares one, as a merged
    /// record overrides only the fields it carries.
    pub const fn refine(&mut self, declared: Option<DateTime<Utc>>) {
        if declared.is_some() {
            self.0 = declared;
        }
    }

    /// Keeps whichever of the held expiry and `observation`'s lies further in the future, as a
    /// series is stored until its last observation is due to expire.
    pub fn extend(&mut self, observation: MappingExpiry) {
        self.0 = self.0.max(observation.0);
    }
}

/// Joins the expiries every mapping contributing to one id carries into the entity's own.
///
/// Each mapping's records are one version of the entity, and clause 4.5.5.3 of ETSI GS CIM 009
/// v1.9.1 resolves an entity-level `expiresAt` that conflicts across versions: when any version
/// lacks one, the entity has none, since expiring it would delete that version's data too;
/// otherwise the instant furthest in the future wins. An id with no mapping has no expiry.
#[must_use]
pub fn join(expiries: impl IntoIterator<Item = MappingExpiry>) -> Option<DateTime<Utc>> {
    expiries
        .into_iter()
        .try_fold(None, |furthest: Option<DateTime<Utc>>, MappingExpiry(declared)| {
            declared.map(|instant| furthest.max(Some(instant)))
        })
        .flatten()
}

/// Joins a series of observations into the entity's expiry: each mapping keeps its observations'
/// furthest expiry ([`MappingExpiry::extend`]), and the mappings are then [`join`]ed.
#[must_use]
pub fn join_observations(observations: impl IntoIterator<Item = (MappingId, MappingExpiry)>) -> Option<DateTime<Utc>> {
    let mut per_mapping: AHashMap<MappingId, MappingExpiry> = AHashMap::new();
    for (mapping_id, observation) in observations {
        per_mapping.entry(mapping_id).or_default().extend(observation);
    }
    join(per_mapping.into_values())
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::mapping_expiry::{MappingExpiry, join, join_observations},
        mapping_id::MappingId,
    };
    use chrono::{DateTime, TimeZone, Utc};
    use pretty_assertions::assert_eq;

    fn instant(seconds: i64) -> Option<DateTime<Utc>> {
        Utc.timestamp_opt(seconds, 0).single()
    }

    #[test]
    fn a_superseding_record_replaces_the_expiry_even_with_none() {
        let mut expiry = MappingExpiry::first(instant(300));
        expiry.supersede(instant(100));
        assert_eq!(expiry, MappingExpiry::first(instant(100)));

        expiry.supersede(None);
        assert_eq!(expiry, MappingExpiry::first(None));
    }

    #[test]
    fn a_merged_record_overrides_the_expiry_only_when_it_declares_one() {
        let mut expiry = MappingExpiry::first(instant(300));
        expiry.refine(instant(100));
        assert_eq!(expiry, MappingExpiry::first(instant(100)));

        expiry.refine(None);
        assert_eq!(expiry, MappingExpiry::first(instant(100)));
    }

    #[test]
    fn a_series_keeps_the_furthest_expiry_and_none_clears_nothing() {
        let mut expiry = MappingExpiry::first(None);
        expiry.extend(MappingExpiry::first(instant(100)));
        expiry.extend(MappingExpiry::first(instant(300)));
        expiry.extend(MappingExpiry::first(None));
        expiry.extend(MappingExpiry::first(instant(200)));

        assert_eq!(expiry, MappingExpiry::first(instant(300)));
    }

    #[test]
    fn mappings_that_all_declare_an_expiry_join_to_the_furthest() {
        assert_eq!(join([MappingExpiry::first(instant(100)), MappingExpiry::first(instant(300))]), instant(300));
    }

    #[test]
    fn one_mapping_without_an_expiry_leaves_the_entity_without_one() {
        assert_eq!(join([MappingExpiry::first(instant(300)), MappingExpiry::first(None)]), None);
        assert_eq!(join([MappingExpiry::first(None), MappingExpiry::first(instant(300))]), None);
    }

    #[test]
    fn no_mapping_joins_to_no_expiry() {
        assert_eq!(join([]), None);
    }

    #[test]
    fn observations_join_per_mapping_before_across_mappings() {
        let one = |seconds: Option<i64>| (MappingId::new(1), MappingExpiry::first(seconds.and_then(instant)));
        let two = |seconds: Option<i64>| (MappingId::new(2), MappingExpiry::first(seconds.and_then(instant)));

        assert_eq!(join_observations([one(Some(100)), two(Some(200)), one(None), one(Some(300))]), instant(300));
        assert_eq!(join_observations([one(Some(100)), two(None), two(None)]), None);
        assert_eq!(join_observations([]), None);
    }
}
