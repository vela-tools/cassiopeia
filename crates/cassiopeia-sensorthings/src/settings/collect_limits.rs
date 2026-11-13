use derive_more::Display;
use getset::CopyGetters;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use thiserror::Error;

/// How far one cycle of a read goes: the most pages a traversal follows and the most Observations
/// the cycle collects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, CopyGetters)]
#[serde(deny_unknown_fields)]
#[getset(get_copy = "pub")]
pub struct CollectLimits {
    /// The most pages one traversal follows.
    #[serde(default)]
    pages: PageLimit,
    /// The most Observations one cycle collects.
    #[serde(default)]
    observations: ObservationLimit,
}

/// The most pages one traversal follows, at least 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, SmartDefault, Serialize, Deserialize)]
#[serde(try_from = "u64")]
pub struct PageLimit(#[default(1000)] u32);

/// The most Observations one cycle collects, at least 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, SmartDefault, Serialize, Deserialize)]
#[serde(try_from = "u64")]
pub struct ObservationLimit(#[default(500_000)] u64);

/// Why a number is no collection limit.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CollectLimitError {
    /// `limits.pages` is zero or past what a traversal can count.
    #[error("`limits.pages` is at least 1 and at most 4294967295, not {0}")]
    Pages(u64),

    /// `limits.observations` is zero.
    #[error("`limits.observations` is at least 1, not 0")]
    Observations,
}

impl PageLimit {
    /// The most pages one traversal follows.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl ObservationLimit {
    /// The most Observations one cycle collects.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for PageLimit {
    type Error = CollectLimitError;

    fn try_from(pages: u64) -> Result<PageLimit, CollectLimitError> {
        u32::try_from(pages)
            .ok()
            .filter(|pages| *pages > 0)
            .map(PageLimit)
            .ok_or(CollectLimitError::Pages(pages))
    }
}

impl TryFrom<u64> for ObservationLimit {
    type Error = CollectLimitError;

    fn try_from(observations: u64) -> Result<ObservationLimit, CollectLimitError> {
        match observations {
            0 => Err(CollectLimitError::Observations),
            observations => Ok(ObservationLimit(observations)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::collect_limits::{CollectLimitError, CollectLimits, ObservationLimit, PageLimit};
    use pretty_assertions::assert_eq;

    #[test]
    fn limits_default_to_a_thousand_pages_and_half_a_million_observations() {
        let limits = CollectLimits::default();

        assert_eq!(limits.pages().get(), 1000);
        assert_eq!(limits.observations().get(), 500_000);
    }

    #[test]
    fn a_limit_of_zero_is_refused() {
        assert_eq!(PageLimit::try_from(0), Err(CollectLimitError::Pages(0)));
        assert_eq!(ObservationLimit::try_from(0), Err(CollectLimitError::Observations));
        assert_eq!(PageLimit::try_from(5_000_000_000), Err(CollectLimitError::Pages(5_000_000_000)));
    }

    #[test]
    fn a_stated_limit_keeps_the_other_at_its_default() {
        let limits: CollectLimits = serde_json::from_str(r#"{"pages": 10}"#).unwrap();

        assert_eq!(limits.pages().get(), 10);
        assert_eq!(limits.observations().get(), 500_000);
    }

    #[test]
    fn an_unknown_limit_is_refused() {
        assert!(serde_json::from_str::<CollectLimits>(r#"{"requests": 10}"#).is_err());
    }
}
