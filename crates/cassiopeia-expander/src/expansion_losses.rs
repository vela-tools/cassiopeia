use cassiopeia_ir::relationship_path::RelationshipPath;
use cassiopeia_unreadable_timestamps::unreadable_expiries::UnreadableExpiries;
use cassiopeia_unresolved_templates::unresolved_templates::UnresolvedTemplates;

/// What one batch's expansion left out while keeping its records, one sink per reason.
///
/// Each reason is a different problem with a different fix, so each keeps its own sink and is
/// reported under its own code; they travel together because every one of them is the same kind of
/// loss, met in the same stage, over the same batch.
#[derive(Default)]
pub struct ExpansionLosses {
    /// Entity `expiresAt` values that read as no instant, so the entity publishes without the member.
    pub expiries: UnreadableExpiries,
    /// Relationships whose source, or whose synthetic entity's identity, failed to render, so the
    /// entity publishes without the link and without the synthetic entity it would have named. Each
    /// is keyed by the relationship's path from the entity.
    pub links: UnresolvedTemplates<RelationshipPath>,
}

impl ExpansionLosses {
    /// Opens a set of empty sinks.
    #[must_use]
    pub fn new() -> ExpansionLosses {
        ExpansionLosses::default()
    }
}

#[cfg(test)]
mod tests {
    use crate::expansion_losses::ExpansionLosses;

    #[test]
    fn a_fresh_set_of_sinks_holds_nothing() {
        let losses = ExpansionLosses::new();

        assert!(losses.expiries.is_empty());
        assert!(losses.links.is_empty());
    }
}
