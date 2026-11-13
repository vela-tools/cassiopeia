use crate::{
    collection_warnings::CollectionWarnings,
    error::CollectorError,
    sensorthings::{
        limit_reached::CollectLimit,
        observation_budget::ObservationBudget,
        page_reader::PageReader,
        snapshot_failure::unwritable,
        traversal::{TraversalEnd, traverse},
    },
};
use cassiopeia_sensorthings::{
    entity_id::EntityId,
    settings::collect_limits::{ObservationLimit, PageLimit},
    snapshot::{line::ObservationLine, writer::ObservationWriter},
};
use cassiopeia_sensorthings_cursor::seen_observations::SeenObservations;
use serde_json::{Map, Value};
use std::{collections::HashSet, io::Write, ops::ControlFlow, path::Path};
use url::Url;

/// Where a read writes its Observations, and how many more it may write.
pub(crate) struct ObservationSink<'a, W> {
    /// The snapshot, past its datastream lines.
    lines: ObservationWriter<W>,
    /// How many more Observations the cycle may write.
    budget: ObservationBudget,
    /// Where the snapshot is written, which a write failure names.
    path: &'a Path,
}

impl<'a, W: Write> ObservationSink<'a, W> {
    /// The sink of `lines`, the snapshot at `path`, that takes at most `limit` Observations.
    pub(crate) const fn new(lines: ObservationWriter<W>, limit: ObservationLimit, path: &'a Path) -> ObservationSink<'a, W> {
        ObservationSink {
            lines,
            budget: ObservationBudget::new(limit),
            path,
        }
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

/// Writes the Observations `request` serves of the data stream `datastream` into `sink`, each once
/// and none of `seen`, as long as its budget allows, following at most `pages` pages, and hands
/// `wrote` each Observation it writes that has an id, before it is written.
///
/// # Errors
///
/// Returns every [`CollectorError`] a traversal fails with, and [`CollectorError::Io`] when the
/// snapshot cannot be written.
pub(crate) fn read_observations<W: Write>(
    reader: &mut PageReader,
    request: Url,
    pages: PageLimit,
    datastream: &EntityId,
    seen: &SeenObservations,
    sink: &mut ObservationSink<'_, W>,
    mut wrote: impl FnMut(&EntityId, &Map<String, Value>),
) -> Result<TraversalEnd, CollectorError> {
    // A `$skip`-built page repeats an item when the server gains rows between pages, so an
    // Observation this traversal already wrote is dropped. One without an id cannot be told apart
    // and is written.
    let mut written: HashSet<EntityId> = HashSet::new();
    traverse(reader, request, pages, |observation| {
        let id = EntityId::of(&observation);
        if let Some(id) = &id
            && (seen.contains(id) || written.contains(id))
        {
            return Ok(ControlFlow::Continue(()));
        }
        if let ControlFlow::Break(limit) = sink.budget.spend() {
            return Ok(ControlFlow::Break(limit));
        }
        if let Some(id) = id {
            wrote(&id, &observation);
            written.insert(id);
        }
        // Each line owns the id of its data stream, while the read keeps it for the next one.
        sink.lines
            .observation(&ObservationLine::new(observation, datastream.clone()))
            .map_err(unwritable(sink.path))?;
        Ok(ControlFlow::Continue(()))
    })
}

/// Records the collection limit that ended one data stream's read, if one did, and says whether the
/// read goes on with the next data stream: a page limit ends only that data stream's read, the
/// observation limit the whole read.
pub(crate) fn next_stream(end: TraversalEnd, warnings: &mut CollectionWarnings) -> ControlFlow<()> {
    let TraversalEnd::LimitReached(limit) = end else {
        return ControlFlow::Continue(());
    };
    let stopped_by = limit.limit();
    warnings.limit_reached(limit);
    match stopped_by {
        CollectLimit::Pages(_) => ControlFlow::Continue(()),
        CollectLimit::Observations(_) => ControlFlow::Break(()),
    }
}
