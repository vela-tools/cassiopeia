use crate::{
    error::CollectorError,
    sensorthings::{
        limit_reached::{CollectLimit, LimitReached},
        page_reader::PageReader,
    },
};
use cassiopeia_sensorthings::{paging::next_link::resolve_next_link, settings::collect_limits::PageLimit};
use serde_json::{Map, Value};
use std::{collections::HashSet, mem};
use url::Url;

/// How a traversal ended.
#[derive(Debug, PartialEq, Eq)]
pub enum TraversalEnd {
    /// The server's last page was read.
    LastPage,
    /// A collection limit stopped the traversal before the last page; what it read is kept.
    LimitReached(LimitReached),
}

/// Follows the request `first` and every next link after it to its end, handing each item of every
/// page to `item` in the order the server sent them.
///
/// Before each request the traversal fails once the run is being stopped, fails on a link to a page
/// it already read, and stops, keeping what it read, once it has read `pages` pages. Each next link
/// is resolved against the URL of the page it came from and must stay on the service root's origin
/// (OGC 18-088 section 9.3.3.6, Req 32), so the input's headers never reach another server.
///
/// # Errors
///
/// Returns, as `E`, [`CollectorError::Interrupted`] once the run is being stopped,
/// [`CollectorError::LinkLoop`] for a link back to a page already read, [`CollectorError::NextLink`]
/// for a next link that is not followed and every error [`PageReader::read`] returns; and every error
/// `item` returns.
pub(crate) fn traverse<E: From<CollectorError>>(
    reader: &mut PageReader,
    first: Url,
    pages: PageLimit,
    mut item: impl FnMut(Map<String, Value>) -> Result<(), E>,
) -> Result<TraversalEnd, E> {
    let mut visited: HashSet<Url> = HashSet::new();
    let mut next = first;
    let mut read: u32 = 0;
    loop {
        reader.ensure_running()?;
        if visited.contains(&next) {
            return Err(CollectorError::LinkLoop { link: Box::new(next) }.into());
        }
        if read == pages.get() {
            // The warning names the root, while the reader keeps its own.
            return Ok(TraversalEnd::LimitReached(LimitReached::new(
                reader.root().clone(),
                CollectLimit::Pages(pages),
                next,
            )));
        }
        let (items, link) = reader.read(&next)?.into_parts();
        items.into_iter().try_for_each(&mut item)?;
        read += 1;
        let Some(link) = link else {
            return Ok(TraversalEnd::LastPage);
        };
        let resolved = resolve_next_link(&next, &link, reader.root()).map_err(|source| CollectorError::NextLink {
            // The error names the page, while the traversal still holds it.
            page: Box::new(next.clone()),
            source,
        })?;
        visited.insert(mem::replace(&mut next, resolved));
    }
}
