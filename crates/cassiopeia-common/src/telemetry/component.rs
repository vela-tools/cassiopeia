//! Typed sub-phases of a stage's service time.

use strum::{Display, IntoStaticStr};

/// A distinct sub-phase of a stage's work, recorded alongside its aggregate service time.
///
/// A component names a genuinely separate slice of a stage's time that is worth surfacing on its own:
/// the resolver's preparation and its two store writes, and the writer's broker request time. It is a
/// typed key rather than a raw string so an emitter and a reader cannot disagree on the spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum StageComponent {
    /// Extracting timestamps, preparing relationship edges, and interning mappings.
    ResolvePrepare,
    /// Writing relationship edges to the relationship store.
    RelationshipStoreWrite,
    /// Writing fragments to the entity store.
    EntityStoreWrite,
    /// The combined relationship and entity store write time.
    StoreWrite,
    /// Time a broker writer spent in the request itself.
    Request,
}

#[cfg(test)]
mod tests {
    use crate::telemetry::component::StageComponent;

    #[test]
    fn a_component_renders_as_its_kebab_case_name() {
        assert_eq!(StageComponent::ResolvePrepare.to_string(), "resolve-prepare");
        assert_eq!(StageComponent::RelationshipStoreWrite.to_string(), "relationship-store-write");
        assert_eq!(StageComponent::Request.to_string(), "request");
    }
}
