use derive_more::Display;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use thiserror::Error;

/// The most items a read asks for in one page.
const MAX_PAGE_SIZE: u16 = 10_000;

/// The `$top` of every request a read sends: how many items it asks for in one page, 1 to 10000.
///
/// A server may serve fewer than asked: a `$top` above the server's own limit is discarded and the
/// server's limit is imposed (OGC 18-088 section 9.3.3.2, Req 26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, SmartDefault, Serialize, Deserialize)]
#[serde(try_from = "u64")]
pub struct PageSize(#[default(1000)] u16);

/// Why a number is no page size.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("`pageSize` is 1 to 10000, not {0}")]
pub struct PageSizeError(u64);

impl PageSize {
    /// The number of items a page is asked for.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u64> for PageSize {
    type Error = PageSizeError;

    fn try_from(size: u64) -> Result<PageSize, PageSizeError> {
        u16::try_from(size)
            .ok()
            .filter(|size| (1..=MAX_PAGE_SIZE).contains(size))
            .map(PageSize)
            .ok_or(PageSizeError(size))
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::page_size::{PageSize, PageSizeError};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_page_size_is_one_to_ten_thousand() {
        assert_eq!(PageSize::try_from(1).unwrap().get(), 1);
        assert_eq!(PageSize::try_from(10_000).unwrap().get(), 10_000);
        assert_eq!(PageSize::try_from(0), Err(PageSizeError(0)));
        assert_eq!(PageSize::try_from(10_001), Err(PageSizeError(10_001)));
        assert_eq!(PageSize::try_from(70_000), Err(PageSizeError(70_000)));
    }

    #[test]
    fn the_default_page_size_is_a_thousand() {
        assert_eq!(PageSize::default().get(), 1000);
    }

    #[test]
    fn a_page_size_is_written_as_its_number() {
        assert_eq!(serde_json::to_string(&PageSize::try_from(200).unwrap()).unwrap(), "200");
        assert_eq!(serde_json::from_str::<PageSize>("200").unwrap().get(), 200);
    }
}
