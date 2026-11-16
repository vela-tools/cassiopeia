/// Where a member that an entity's schema requires at its top level is decided.
///
/// An entity checked on its own is the entity a run writes, so a member it lacks is missing from
/// what the run writes. An entity that is one of several a run folds into the entity it writes is
/// not: another of them may hold the member, and the entity written holds it as soon as one does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequiredMembers {
    /// On the entity checked, which is the entity the run writes.
    Decided,
    /// On the entity the run folds the checked entity into: a member the checked entity lacks is
    /// not a violation of it, and is carried with its outcome to the stage that folds it.
    Carried,
}
