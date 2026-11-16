use crate::{
    error::CollectorError,
    sensorthings::{
        collection_listing::{CollectionListing, read_listing},
        page_reader::PageReader,
    },
};
use cassiopeia_sensorthings::{request::presence_request::any_stream_request, service_root::ServiceRoot, stream_kind::StreamKind};
use getset::Getters;
use tracing::debug;

/// A server that holds `MultiDatastreams` an input reads none of, because its `multiDatastreams`
/// setting selects none.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Getters)]
pub struct MultiDatastreamsUnread {
    /// The service root of the server.
    #[getset(get = "pub")]
    root: ServiceRoot,
}

impl MultiDatastreamsUnread {
    /// The `MultiDatastreams` of `root` an input leaves unread.
    #[must_use]
    pub const fn new(root: ServiceRoot) -> MultiDatastreamsUnread {
        MultiDatastreamsUnread { root }
    }

    /// Checks whether the server `reader` reads holds `MultiDatastreams`, in at most two requests: its
    /// service root, and, when the root lists the `MultiDatastreams` collection, at most one
    /// `MultiDatastream`. Returns what it found, or `None` when the server holds none.
    ///
    /// The input never asked for the check, so a check that fails reports nothing and fails nothing;
    /// a stop of the run ends the cycle at its next request.
    pub(crate) fn check(reader: &mut PageReader) -> Option<MultiDatastreamsUnread> {
        match held(reader) {
            Ok(found) => found,
            Err(error) => {
                debug!("The check of `{}` for MultiDatastreams failed, so it reports nothing: {error}", reader.root());
                None
            }
        }
    }
}

/// The `MultiDatastreams` the server `reader` reads holds, or `None` when it holds none.
fn held(reader: &mut PageReader) -> Result<Option<MultiDatastreamsUnread>, CollectorError> {
    match read_listing(reader, StreamKind::MultiDatastream)? {
        CollectionListing::Unlisted => return Ok(None),
        CollectionListing::Listed => {}
    }
    let request = any_stream_request(reader.root(), StreamKind::MultiDatastream);
    let (multi_datastreams, _) = reader.read(&request)?.into_parts();
    if multi_datastreams.is_empty() {
        return Ok(None);
    }
    // The finding names the root, while the reader keeps its own.
    Ok(Some(MultiDatastreamsUnread::new(reader.root().clone())))
}
