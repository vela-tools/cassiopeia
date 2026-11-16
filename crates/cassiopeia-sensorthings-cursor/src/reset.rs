use crate::{cursor_store_error::CursorStoreError, positions::Positions};
use cassiopeia_sensorthings::{
    cursor_name::CursorName,
    entity_id::EntityId,
    read_stream::{ReadStream, ReadStreamKind},
    stream_id::StreamId,
    stream_kind::StreamKind,
};
use chrono::{DateTime, Utc};
use derive_more::{Display, From};
use std::collections::BTreeMap;

/// A data stream or Thing id as a person writes it, such as `5`, which names the stored ids of one
/// kind of stream whose text form it is.
///
/// A server chooses whether its ids are integers or texts, which a person cannot see, so `5` names
/// the integer id 5 and the text id `"5"` alike. Any text is an id text, one the cursor holds no
/// position for included, so the command line reads it from `String`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, From)]
#[from(Box<str>, String)]
pub struct IdText(Box<str>);

/// What a reset applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetScope {
    /// Every position of the cursor.
    Store,
    /// The position of the Datastream the id names.
    Datastream(IdText),
    /// The position of the `MultiDatastream` the id names.
    MultiDatastream(IdText),
    /// The position of the Thing the id names.
    Thing(IdText),
}

/// Where a reset leaves the positions it applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetTarget {
    /// Removes them, so their streams start at the input's initial point again.
    InitialPoint,
    /// Sets their high-water marks to the instant and forgets the entities they have seen.
    HighWater(DateTime<Utc>),
}

impl IdText {
    /// Whether this names `id`.
    fn names(&self, id: &EntityId) -> bool {
        id.to_string() == *self.0
    }
}

impl ResetScope {
    /// Whether the reset applies to the position of `stream`.
    fn covers(&self, stream: &ReadStream) -> bool {
        match (self, stream) {
            (ResetScope::Store, _) => true,
            (ResetScope::Datastream(text), ReadStream::Data(StreamId::Datastream(id)))
            | (ResetScope::MultiDatastream(text), ReadStream::Data(StreamId::MultiDatastream(id)))
            | (ResetScope::Thing(text), ReadStream::Thing(id)) => text.names(id),
            (ResetScope::Datastream(_), ReadStream::Data(StreamId::MultiDatastream(_)) | ReadStream::Thing(_))
            | (ResetScope::MultiDatastream(_), ReadStream::Data(StreamId::Datastream(_)) | ReadStream::Thing(_))
            | (ResetScope::Thing(_), ReadStream::Data(_)) => false,
        }
    }

    /// The kind and id of the one stream the reset names, or `None` for a reset of every position.
    const fn named(&self) -> Option<(ReadStreamKind, &IdText)> {
        match self {
            ResetScope::Store => None,
            ResetScope::Datastream(text) => Some((ReadStreamKind::Data(StreamKind::Datastream), text)),
            ResetScope::MultiDatastream(text) => Some((ReadStreamKind::Data(StreamKind::MultiDatastream), text)),
            ResetScope::Thing(text) => Some((ReadStreamKind::Thing, text)),
        }
    }
}

/// `positions`, the positions of the cursor `cursor`, reset within `scope` to `target`.
///
/// # Errors
///
/// Returns [`CursorStoreError::UnknownStream`] when `scope` names a data stream or a Thing the cursor
/// holds no position for.
pub(crate) fn reset(positions: Positions, scope: &ResetScope, target: ResetTarget, cursor: &CursorName) -> Result<Positions, CursorStoreError> {
    let mut positions = BTreeMap::from(positions);
    if let Some((kind, text)) = scope.named()
        && !positions.keys().any(|stream| scope.covers(stream))
    {
        return Err(CursorStoreError::UnknownStream {
            // The error outlives the store it names.
            cursor: cursor.clone(),
            kind,
            id: text.clone(),
        });
    }
    match target {
        ResetTarget::InitialPoint => positions.retain(|stream, _| !scope.covers(stream)),
        ResetTarget::HighWater(instant) => positions
            .iter_mut()
            .filter(|(stream, _)| scope.covers(stream))
            .for_each(|(_, position)| position.rewind(instant)),
    }
    Ok(Positions::from(positions))
}

#[cfg(test)]
mod tests {
    use crate::reset::{IdText, ResetScope};
    use cassiopeia_sensorthings::{entity_id::EntityId, read_stream::ReadStream, stream_id::StreamId};

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
    fn an_id_text_is_made_from_owned_text() {
        assert_eq!(IdText::from("t-5".to_owned()), text("t-5"));
    }

    #[test]
    fn a_store_reset_covers_every_stream_and_a_stream_reset_its_own() {
        let datastream = |id: &str| ReadStream::Data(StreamId::Datastream(EntityId::Text(id.into())));
        let multi = |id: &str| ReadStream::Data(StreamId::MultiDatastream(EntityId::Text(id.into())));
        let thing = |id: &str| ReadStream::Thing(EntityId::Text(id.into()));

        assert!(ResetScope::Store.covers(&datastream("t-1")));
        assert!(ResetScope::Store.covers(&multi("t-1")));
        assert!(ResetScope::Store.covers(&thing("t-1")));
        assert!(ResetScope::Thing(text("t-1")).covers(&thing("t-1")));
        assert!(!ResetScope::Thing(text("t-1")).covers(&thing("t-2")));
        assert!(!ResetScope::Thing(text("t-1")).covers(&datastream("t-1")));
        assert!(!ResetScope::Datastream(text("t-1")).covers(&thing("t-1")));
        assert!(!ResetScope::MultiDatastream(text("t-1")).covers(&thing("t-1")));
        assert!(ResetScope::Datastream(text("t-1")).covers(&datastream("t-1")));
        assert!(!ResetScope::Datastream(text("t-1")).covers(&datastream("t-2")));
        assert!(!ResetScope::Datastream(text("t-1")).covers(&multi("t-1")));
        assert!(ResetScope::MultiDatastream(text("t-1")).covers(&multi("t-1")));
        assert!(!ResetScope::MultiDatastream(text("t-1")).covers(&datastream("t-1")));
    }
}
