use crate::{
    collection_warnings::CollectionWarnings,
    error::CollectorError,
    sensorthings::{
        feature_of_interest_check::FeatureOfInterestCheck,
        historical_location_read::HistoricalLocationSink,
        limit_reached::CollectLimit,
        observation_budget::ObservationBudget,
        page_reader::PageReader,
        snapshot_failure::unwritable,
        traversal::{TraversalEnd, traverse_unseen},
    },
};
use cassiopeia_sensorthings::{
    entity_id::EntityId,
    service::SensorThingsService,
    settings::collect_limits::PageLimit,
    snapshot::{line::ObservationLine, writer::ObservationWriter},
    stream_id::StreamId,
};
use cassiopeia_sensorthings_cursor::seen_entities::SeenEntities;
use serde_json::{Map, Value};
use std::{io::Write, ops::ControlFlow, path::Path};
use url::Url;

/// Where a read writes its Observations, how many more it may write, and which it wrote without the
/// `FeatureOfInterest` it asked for.
pub(crate) struct ObservationSink<'a, W> {
    /// The snapshot, past its data stream lines.
    lines: ObservationWriter<W>,
    /// How many more Observations the cycle may write.
    budget: ObservationBudget,
    /// The count of the Observations written without the `FeatureOfInterest` the read asked for.
    features: FeatureOfInterestCheck<'a>,
    /// Where the snapshot is written, which a write failure names.
    path: &'a Path,
}

impl<'a, W: Write> ObservationSink<'a, W> {
    /// The sink of `lines`, the snapshot at `path`, of a read of `service`, that takes at most the
    /// Observations its `limits.observations` allows.
    pub(crate) fn new(lines: ObservationWriter<W>, service: &'a SensorThingsService, path: &'a Path) -> ObservationSink<'a, W> {
        ObservationSink {
            lines,
            budget: ObservationBudget::new(service.settings().limits().observations()),
            features: FeatureOfInterestCheck::new(service),
            path,
        }
    }

    /// Ends the Observations, so the `HistoricalLocations` can follow them, and records those written
    /// without the `FeatureOfInterest` the read asked for in `warnings`.
    pub(crate) fn into_historical_locations(self, warnings: &mut CollectionWarnings) -> HistoricalLocationSink<'a, W> {
        self.features.record(warnings);
        HistoricalLocationSink::new(self.lines.into_historical_locations(), self.path)
    }
}

/// Writes the Observations `request` serves of the data stream `datastream` into `sink`, each once
/// and none of `seen`, as long as its budget allows, following at most `pages` pages, counts each
/// it writes without the `FeatureOfInterest` the read asked for, and hands `wrote` each Observation
/// it writes that has an id, before it is written.
///
/// # Errors
///
/// Returns every [`CollectorError`] a traversal fails with, and [`CollectorError::Io`] when the
/// snapshot cannot be written.
pub(crate) fn read_observations<W: Write>(
    reader: &mut PageReader,
    request: Url,
    pages: PageLimit,
    datastream: &StreamId,
    seen: &SeenEntities,
    sink: &mut ObservationSink<'_, W>,
    mut wrote: impl FnMut(&EntityId, &Map<String, Value>),
) -> Result<TraversalEnd, CollectorError> {
    traverse_unseen(reader, request, pages, seen, |id, observation| {
        if let ControlFlow::Break(limit) = sink.budget.spend() {
            return Ok(ControlFlow::Break(limit));
        }
        sink.features.observation(datastream, &observation);
        if let Some(id) = id {
            wrote(id, &observation);
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
