use serde_json::{Map, Value};
use thiserror::Error;

/// The member of a collection response that holds its items (OGC 18-088 section 9.2.1).
const VALUE: &str = "value";

/// The member of a partial collection response that links its next page (OGC 18-088 section
/// 9.3.3.6, Req 32).
const NEXT_LINK: &str = "@iot.nextLink";

/// One page of a collection response: its items, in the order the server sent them, and the link
/// to the next page, which the last page has none of.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// The entities on the page, each as the server sent it.
    items: Vec<Map<String, Value>>,
    /// The `@iot.nextLink`, as the server wrote it.
    next_link: Option<Box<str>>,
}

/// Why a response body is no page of a collection.
#[derive(Debug, Error)]
pub enum PageError {
    /// The body is not JSON.
    #[error("the body is not JSON")]
    NotJson(#[source] serde_json::Error),

    /// The body holds no `value` array.
    #[error("the body is no collection: it holds no `value` array")]
    NotACollection,

    /// The body's `@iot.nextLink` is not a string.
    #[error("the body's `@iot.nextLink` is not a string")]
    NextLinkNotText,

    /// An item of `value` is no JSON object, so it is no entity.
    #[error("item {position} of `value` is no JSON object")]
    ItemNotAnObject {
        /// The item's position in `value`, counted from 1.
        position: usize,
    },
}

impl Page {
    /// Reads a response body as one page, ignoring every member but `value` and `@iot.nextLink`,
    /// such as `@iot.count`.
    ///
    /// # Errors
    ///
    /// Returns the [`PageError`] that names what the body is not.
    pub fn from_body(body: &[u8]) -> Result<Page, PageError> {
        let Value::Object(mut members) = serde_json::from_slice(body).map_err(PageError::NotJson)? else {
            return Err(PageError::NotACollection);
        };
        let Some(Value::Array(values)) = members.remove(VALUE) else {
            return Err(PageError::NotACollection);
        };
        let next_link = match members.remove(NEXT_LINK) {
            None => None,
            Some(Value::String(link)) => Some(link.into_boxed_str()),
            Some(_) => return Err(PageError::NextLinkNotText),
        };
        let items = values
            .into_iter()
            .enumerate()
            .map(|(index, item)| match item {
                Value::Object(entity) => Ok(entity),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => Err(PageError::ItemNotAnObject { position: index + 1 }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page { items, next_link })
    }

    /// The link to the next page, as the server wrote it; `None` on the last page.
    #[must_use]
    pub fn next_link(&self) -> Option<&str> {
        self.next_link.as_deref()
    }

    /// Consumes the page and returns its items and its next link.
    #[must_use]
    pub fn into_parts(self) -> (Vec<Map<String, Value>>, Option<Box<str>>) {
        (self.items, self.next_link)
    }
}

#[cfg(test)]
mod tests {
    use crate::paging::page::{Page, PageError};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn a_page_yields_its_items_and_next_link() {
        let page = Page::from_body(br#"{"value":[{"@iot.id":1}],"@iot.nextLink":"https://h.example/v1.1/Datastreams?$skip=1"}"#).unwrap();

        assert_eq!(page.next_link(), Some("https://h.example/v1.1/Datastreams?$skip=1"));
        let (items, _) = page.into_parts();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["@iot.id"], json!(1));
    }

    #[test]
    fn the_last_page_has_no_next_link() {
        let page = Page::from_body(br#"{"value":[],"@iot.count":0}"#).unwrap();

        assert_eq!(page.next_link(), None);
        assert_eq!(page.into_parts().0.len(), 0);
    }

    #[test]
    fn a_body_that_is_no_collection_is_refused() {
        assert!(matches!(Page::from_body(br#"{"value":5}"#), Err(PageError::NotACollection)));
        assert!(matches!(Page::from_body(b"[1]"), Err(PageError::NotACollection)));
        assert!(matches!(Page::from_body(b"not json"), Err(PageError::NotJson(_))));
    }

    #[test]
    fn a_next_link_that_is_no_string_is_refused() {
        assert!(matches!(Page::from_body(br#"{"value":[],"@iot.nextLink":7}"#), Err(PageError::NextLinkNotText)));
    }

    #[test]
    fn an_item_that_is_no_object_is_refused() {
        let error = Page::from_body(br#"{"value":[{"@iot.id":1},2]}"#).unwrap_err();

        assert!(matches!(error, PageError::ItemNotAnObject { position: 2 }), "{error}");
        assert_eq!(error.to_string(), "item 2 of `value` is no JSON object");
    }
}
