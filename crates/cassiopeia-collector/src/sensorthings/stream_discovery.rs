use crate::{
    error::CollectorError,
    sensorthings::{
        page_reader::PageReader,
        traversal::{TraversalEnd, traverse},
    },
};
use cassiopeia_sensorthings::{
    service::SensorThingsService,
    service_root::ServiceRoot,
    settings::sensorthings_settings::SensorThingsSettings,
    stream_kind::StreamKind,
};
use serde_json::{Map, Value};
use std::ops::ControlFlow;
use url::Url;

/// Follows the discovery `request` builds for every kind of data stream `service` reads, Datastreams
/// first, each through its own traversal, and hands every data stream served to `datastream` with
/// its kind, in the order the server sent them. Returns each kind with how its discovery ended.
///
/// # Errors
///
/// Returns, as `E`, every [`CollectorError`] a traversal fails with, and every error `datastream`
/// returns, which ends the discovery before its next request.
pub(crate) fn discover_streams<E: From<CollectorError>>(
    reader: &mut PageReader,
    service: &SensorThingsService,
    request: fn(&ServiceRoot, &SensorThingsSettings, StreamKind) -> Url,
    mut datastream: impl FnMut(StreamKind, Map<String, Value>) -> Result<(), E>,
) -> Result<Vec<(StreamKind, TraversalEnd)>, E> {
    let settings = service.settings();
    settings
        .kinds()
        .map(|kind| {
            let end = traverse(reader, request(service.root(), settings, kind), settings.limits().pages(), |served| {
                datastream(kind, served).map(|()| ControlFlow::Continue(()))
            })?;
            Ok((kind, end))
        })
        .collect()
}
