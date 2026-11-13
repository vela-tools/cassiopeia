use crate::{cursor_store_error::CursorStoreError, positions::Positions};
use cassiopeia_sensorthings::{cursor_name::CursorName, entity_id::EntityId};
use chrono::{DateTime, Utc};
use derive_more::{Display, From};
use std::collections::BTreeMap;

/// A data stream id as a person writes it, such as `5`, which names the stored ids whose text form it
/// is.
///
/// A server chooses whether its ids are integers or texts, which a person cannot see, so `5` names
/// the integer id 5 and the text id `"5"` alike.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, From)]
pub struct IdText(Box<str>);

/// What a reset applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetScope {
    /// Every position of the cursor.
    Store,
    /// The position of the data stream the id names.
    Datastream(IdText),
}

/// Where a reset leaves the positions it applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetTarget {
    /// Removes them, so their data streams start at the input's initial point again.
    InitialPoint,
    /// Sets their high-water marks to the instant and forgets the Observations they have seen.
    HighWater(DateTime<Utc>),
}

impl IdText {
    /// Whether this names `id`.
    fn names(&self, id: &EntityId) -> bool {
        id.to_string() == *self.0
    }
}

impl ResetScope {
    /// Whether the reset applies to the position of `datastream`.
    fn covers(&self, datastream: &EntityId) -> bool {
        match self {
            ResetScope::Store => true,
            ResetScope::Datastream(text) => text.names(datastream),
        }
    }
}

/// `positions`, the positions of the cursor `cursor`, reset within `scope` to `target`.
///
/// # Errors
///
/// Returns [`CursorStoreError::UnknownDatastream`] when `scope` names a data stream the cursor holds
/// no position for.
pub(crate) fn reset(positions: Positions, scope: &ResetScope, target: ResetTarget, cursor: &CursorName) -> Result<Positions, CursorStoreError> {
    let mut positions = BTreeMap::from(positions);
    if let ResetScope::Datastream(text) = scope
        && !positions.keys().any(|datastream| text.names(datastream))
    {
        return Err(CursorStoreError::UnknownDatastream {
            // The error outlives the store it names.
            cursor: cursor.clone(),
            datastream: text.clone(),
        });
    }
    match target {
        ResetTarget::InitialPoint => positions.retain(|datastream, _| !scope.covers(datastream)),
        ResetTarget::HighWater(instant) => positions
            .iter_mut()
            .filter(|(datastream, _)| scope.covers(datastream))
            .for_each(|(_, position)| position.rewind(instant)),
    }
    Ok(Positions::from(positions))
}

#[cfg(test)]
mod tests {
    use crate::reset::{IdText, ResetScope};
    use cassiopeia_sensorthings::entity_id::EntityId;

    fn text(id: &str) -> IdText {
        IdText::from(Box::<str>::from(id))
    }

    #[test]
    fn an_id_written_as_text_names_integer_and_text_ids_alike() {
        assert!(text("5").names(&EntityId::Integer(5)));
        assert!(text("5").names(&EntityId::Text("5".into())));
        assert!(!text("05").names(&EntityId::Integer(5)));
        assert!(!text("5").names(&EntityId::Integer(56)));
    }

    #[test]
    fn a_store_reset_covers_every_data_stream_and_a_stream_reset_its_own() {
        assert!(ResetScope::Store.covers(&EntityId::Text("t-1".into())));
        assert!(ResetScope::Datastream(text("t-1")).covers(&EntityId::Text("t-1".into())));
        assert!(!ResetScope::Datastream(text("t-1")).covers(&EntityId::Text("t-2".into())));
    }
}
