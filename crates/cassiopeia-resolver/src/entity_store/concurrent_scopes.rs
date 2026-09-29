use ahash::RandomState;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use dashmap::{DashMap, Entry};
use std::sync::Arc;
use urn_rs::Urn;

/// Each base id's merged scope set, shared by every clone of an in-memory entity store.
///
/// Every fragment of an id may declare its own scopes, and clause 4.18 of ETSI GS CIM 009 v1.9.1
/// merges them when representations of one entity are combined, so a later fragment extends the set
/// rather than replacing it.
#[derive(Clone, Debug, Default)]
pub(crate) struct ConcurrentScopes(Arc<DashMap<Urn, NgsiLdScope, RandomState>>);

impl ConcurrentScopes {
    /// Merges one fragment's scopes into the set held for `base_id`.
    pub(crate) fn merge(&self, base_id: Urn, scope: NgsiLdScope) {
        match self.0.entry(base_id) {
            Entry::Occupied(mut held) => {
                held.get_mut().merge(scope);
            }
            Entry::Vacant(vacant) => {
                vacant.insert(scope);
            }
        }
    }

    /// The merged scope set held for `base_id`, or `None` when none of its fragments had one.
    pub(crate) fn get(&self, base_id: &Urn) -> Option<NgsiLdScope> {
        // The store keeps the set for later assemblies of the id, so the caller gets its own copy.
        self.0.get(base_id).map(|held| held.clone())
    }

    /// Forgets every held scope set.
    pub(crate) fn clear(&self) {
        self.0.clear();
    }
}
