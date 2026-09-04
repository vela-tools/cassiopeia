use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// A backing store the resolver keeps its state in.
///
/// The entity store and relationship store each use this enum to select their backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
pub enum StoreKind {
    /// An in-memory concurrent hash map: the fastest option, and the one that uses the most memory.
    #[serde(rename = "dashmap")]
    #[value(name = "dashmap")]
    #[default]
    DashMap,

    /// A disk-backed embedded store, for runs whose count does not fit in memory.
    #[value(name = "redb")]
    Redb,
}

#[cfg(test)]
mod tests {
    use crate::store_kind::StoreKind;

    #[test]
    fn the_wire_form_is_kebab_case() {
        assert_eq!(serde_json::to_string(&StoreKind::DashMap).unwrap(), r#""dashmap""#);
        assert_eq!(serde_json::from_str::<StoreKind>(r#""redb""#).unwrap(), StoreKind::Redb);
    }

    #[test]
    fn the_unstated_store_is_held_in_memory() {
        assert_eq!(StoreKind::default(), StoreKind::DashMap);
    }
}
