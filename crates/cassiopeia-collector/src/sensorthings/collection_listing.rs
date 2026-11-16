use crate::{downloader::Downloader, error::CollectorError, sensorthings::page_reader::PageReader};
use cassiopeia_sensorthings::{
    request::presence_request::service_document_request,
    service::SensorThingsService,
    service_document::lists_collection,
    stream_kind::StreamKind,
};
use std::sync::atomic::AtomicBool;

/// Whether a server's service root lists a collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionListing {
    /// The service root lists the collection.
    Listed,
    /// The service root does not list the collection, so the server does not serve it.
    Unlisted,
}

/// Whether the service root of `service` lists the collection of the data streams of `kind`, read
/// through `downloader` as a run reads a server, with the input's delay, and stopping once
/// `shutdown` is set.
///
/// # Errors
///
/// Returns every [`CollectorError`] reading a page fails with: a stop, a failed request, and a
/// service root document that is no collection of the collections the server serves.
pub fn collection_listing(
    service: &SensorThingsService,
    kind: StreamKind,
    downloader: Downloader,
    shutdown: &'static AtomicBool,
) -> Result<CollectionListing, CollectorError> {
    // The reader names the root in its failures, while the service keeps its own.
    let mut reader = PageReader::new(service.root().clone(), downloader, service.settings().delay(), shutdown);
    read_listing(&mut reader, kind)
}

/// Whether the service root `reader` reads lists the collection of the data streams of `kind`.
///
/// # Errors
///
/// Returns every [`CollectorError`] [`PageReader::read`] returns.
pub(crate) fn read_listing(reader: &mut PageReader, kind: StreamKind) -> Result<CollectionListing, CollectorError> {
    let request = service_document_request(reader.root());
    let (collections, _) = reader.read(&request)?.into_parts();
    if lists_collection(&collections, kind) {
        Ok(CollectionListing::Listed)
    } else {
        Ok(CollectionListing::Unlisted)
    }
}
