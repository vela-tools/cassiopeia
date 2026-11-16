use cassiopeia_common::stage::Stage;
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use cassiopeia_validator::{carried_members::CarriedMembers, required_members::RequiredMembers};

/// What the validator stage forwards for each entity it lets through, which also fixes where the
/// members an entity's schema requires at the top level are decided, and the stage that receives it.
pub(crate) trait Forwarded: Send + 'static {
    /// Where the validator decides the members an entity's schema requires at the top level.
    const REQUIRED_MEMBERS: RequiredMembers;

    /// The stage the validator forwards to.
    const DOWNSTREAM: Stage;

    /// What the validator forwards for `entity`, which it lets through carrying `carried`.
    fn forward(entity: NgsiLdEntity, carried: CarriedMembers) -> Self;
}

/// A current-state run writes each entity the validator lets through: the entity checked is the
/// entity written, so its required members are decided on it, and it goes straight to the writer.
impl Forwarded for NgsiLdEntity {
    const REQUIRED_MEMBERS: RequiredMembers = RequiredMembers::Decided;
    const DOWNSTREAM: Stage = Stage::Writer;

    fn forward(entity: NgsiLdEntity, _carried: CarriedMembers) -> NgsiLdEntity {
        // A check that decides the required members on the entity carries none of them.
        entity
    }
}
