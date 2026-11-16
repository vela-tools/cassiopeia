use crate::stages::forwarded::Forwarded;
use cassiopeia_common::stage::Stage;
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use cassiopeia_validator::{carried_members::CarriedMembers, required_members::RequiredMembers};

/// The entity of one record of a series run, as the validator lets it through: one instance of the
/// entity the run writes, with the members its schema requires at the top level that it lacks.
///
/// Another record of the same entity may hold such a member, so the fold decides them once it holds
/// every record of the entity.
pub(crate) struct RecordEntity {
    /// The record's entity.
    pub(crate) entity: NgsiLdEntity,
    /// The members its schema requires at the top level that it lacks.
    pub(crate) carried: CarriedMembers,
}

/// A series run folds each entity the validator lets through, with the other records of its id, into
/// the entity it writes, so the members a schema requires at the top level are decided by the fold.
impl Forwarded for RecordEntity {
    const REQUIRED_MEMBERS: RequiredMembers = RequiredMembers::Carried;
    const DOWNSTREAM: Stage = Stage::Aggregator;

    fn forward(entity: NgsiLdEntity, carried: CarriedMembers) -> RecordEntity {
        RecordEntity { entity, carried }
    }
}
