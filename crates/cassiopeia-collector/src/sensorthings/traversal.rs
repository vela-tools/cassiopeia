use crate::{
    error::CollectorError,
    sensorthings::{
        limit_reached::{CollectLimit, LimitReached},
        page_reader::PageReader,
    },
};
use cassiopeia_sensorthings::{entity_id::EntityId, paging::next_link::resolve_next_link, settings::collect_limits::PageLimit};
use cassiopeia_sensorthings_cursor::seen_entities::SeenEntities;
use serde_json::{Map, Value};
use std::{collections::HashSet, mem, ops::ControlFlow};
use url::Url;

/// How a traversal ended.
#[derive(Debug, PartialEq, Eq)]
pub enum TraversalEnd {
    /// The server's last page was read.
    LastPage,
    /// A collection limit stopped the traversal before it read everything; what it read is kept.
    LimitReached(LimitReached),
}

/// Follows the request `first` and every next link after it to its end, handing each item of every
/// page to `item` in the order the server sent them.
///
/// Before each request the traversal fails once the run is being stopped, fails on a link to a page
/// it already read, and stops, keeping what it read, once it has read `pages` pages. Each next link
/// is resolved against the URL of the page it came from and must stay on the service root's origin
/// (OGC 18-088 section 9.3.3.6, Req 32), so the input's headers never reach another server. `item`
/// may stop the traversal with the collection limit it reached, which then names the page the item
/// came from.
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
    mut item: impl FnMut(Map<String, Value>) -> Result<ControlFlow<CollectLimit>, E>,
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
        for served in items {
            if let ControlFlow::Break(limit) = item(served)? {
                // The warning names the root, while the reader keeps its own.
                return Ok(TraversalEnd::LimitReached(LimitReached::new(reader.root().clone(), limit, next)));
            }
        }
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

/// Follows the request `first` as [`traverse`] does, handing `item` each item none of `seen` holds
/// and this traversal has not handed it before, with its id when it has one.
///
/// A `$skip`-built page repeats an item when the server gains rows between pages, so an item this
/// traversal already handed is dropped, as is one a cursor position has seen. One without an id
/// cannot be told apart and is handed every time.
///
/// # Errors
///
/// Returns every error [`traverse`] returns, and every error `item` returns.
pub(crate) fn traverse_unseen(
    reader: &mut PageReader,
    first: Url,
    pages: PageLimit,
    seen: &SeenEntities,
    mut item: impl FnMut(Option<&EntityId>, Map<String, Value>) -> Result<ControlFlow<CollectLimit>, CollectorError>,
) -> Result<TraversalEnd, CollectorError> {
    let mut handed: HashSet<EntityId> = HashSet::new();
    traverse(reader, first, pages, |served| {
        let id = EntityId::of(&served);
        if let Some(id) = &id
            && (seen.contains(id) || handed.contains(id))
        {
            return Ok(ControlFlow::Continue(()));
        }
        let flow = item(id.as_ref(), served)?;
        if let Some(id) = id {
            handed.insert(id);
        }
        Ok(flow)
    })
}
