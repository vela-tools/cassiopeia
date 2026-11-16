use crate::{
    collection_warnings::CollectionWarnings,
    error::CollectorError,
    sensorthings::{
        page_reader::PageReader,
        snapshot_failure::unwritable,
        traversal::{TraversalEnd, traverse_unseen},
    },
};
use cassiopeia_sensorthings::{
    entity_id::EntityId,
    historical_location_time::historical_location_time,
    read_window::ReadWindow,
    request::historical_locations_request::historical_locations_request,
    service::SensorThingsService,
    snapshot::{line::HistoricalLocationLine, writer::HistoricalLocationWriter},
};
use cassiopeia_sensorthings_cursor::seen_entities::SeenEntities;
use std::{io::Write, ops::ControlFlow, path::Path};

/// Where a read writes its `HistoricalLocations`: the snapshot, past its observation lines.
///
/// `limits.observations` counts Observations only, so a sink takes every `HistoricalLocation` it is
/// handed.
pub(crate) struct HistoricalLocationSink<'a, W> {
    /// The snapshot, past its observation lines.
    lines: HistoricalLocationWriter<W>,
    /// Where the snapshot is written, which a write failure names.
    path: &'a Path,
}

impl<'a, W: Write> HistoricalLocationSink<'a, W> {
    /// The sink of `lines`, the snapshot at `path`.
    pub(crate) const fn new(lines: HistoricalLocationWriter<W>, path: &'a Path) -> HistoricalLocationSink<'a, W> {
        HistoricalLocationSink { lines, path }
    }

    /// Ends the snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`CollectorError::Io`] when the snapshot cannot be written.
    pub(crate) fn finish(self) -> Result<(), CollectorError> {
        self.lines.finish().map(drop).map_err(unwritable(self.path))
    }
}

/// Writes the `HistoricalLocations` of the Thing `thing` whose `time` falls in `window` into `sink`,
/// oldest first, each with its Locations, each once and none of `seen`, and returns those it wrote
/// that have an id and a `time` that names an instant, each with that instant, for a cursor to move
/// past.
///
/// The request is followed under the paging rules, the pacing and `limits.pages` of every request of
/// `service`'s read, and a page limit that stops it is recorded in `warnings`.
///
/// # Errors
///
/// Returns every [`CollectorError`] a traversal fails with, and [`CollectorError::Io`] when the
/// snapshot cannot be written.
pub(crate) fn read_historical_locations<W: Write>(
    reader: &mut PageReader,
    service: &SensorThingsService,
    thing: &EntityId,
    window: ReadWindow,
    seen: &SeenEntities,
    sink: &mut HistoricalLocationSink<'_, W>,
    warnings: &mut CollectionWarnings,
) -> Result<SeenEntities, CollectorError> {
    let settings = service.settings();
    let request = historical_locations_request(service.root(), thing, window, settings.page_size());
    let mut written = SeenEntities::default();
    let end = traverse_unseen(reader, request, settings.limits().pages(), seen, |id, historical_location| {
        if let Some(id) = id
            && let Some(time) = historical_location_time(&historical_location)
        {
            // The record of what was written keeps the id, while the traversal keeps it to drop
            // repeats.
            written.insert(id.clone(), time);
        }
        // Each line owns the id of its Thing, while the read keeps it for the next one.
        sink.lines
            .historical_location(&HistoricalLocationLine::new(historical_location, thing.clone()))
            .map_err(unwritable(sink.path))?;
        Ok(ControlFlow::Continue(()))
    })?;
    if let TraversalEnd::LimitReached(limit) = end {
        warnings.limit_reached(limit);
    }
    Ok(written)
}
