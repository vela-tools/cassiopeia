use cassiopeia_common::store_kind::StoreKind;
use serde::{Deserialize, Serialize};

/// Where the resolver keeps the state it needs to link entities to one another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Resolver {
    /// The store holding already-seen entities.
    pub entity_store: StoreKind,

    /// The store holding relationship targets that have not been seen yet.
    pub relationship_store: StoreKind,
}

#[cfg(test)]
mod tests {
    use crate::resolver::Resolver;
    use cassiopeia_common::store_kind::StoreKind;

    #[test]
    fn resolution_state_is_held_in_memory_by_default() {
        let resolver = Resolver::default();

        assert_eq!(resolver.entity_store, StoreKind::DashMap);
        assert_eq!(resolver.relationship_store, StoreKind::DashMap);
    }

    #[test]
    fn reads_disk_backed_stores() {
        let resolver: Resolver = toml::from_str("entity_store = \"redb\"\nrelationship_store = \"redb\"").unwrap();

        assert_eq!(resolver.entity_store, StoreKind::Redb);
        assert_eq!(resolver.relationship_store, StoreKind::Redb);
    }
}
