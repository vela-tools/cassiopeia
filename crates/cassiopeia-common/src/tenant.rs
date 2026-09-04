use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// A newtype for the NGSILD-Tenant header value.
///
/// A tenant is always non-empty: the constructor rejects the empty string so an empty tenant can
/// never reach a broker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Display)]
#[serde(transparent)]
#[display("{_0}")]
pub struct Tenant(String);

impl Tenant {
    /// Creates a new `Tenant`, returning `None` if the string is empty.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Option<Tenant> {
        let s = value.into();
        if s.is_empty() { None } else { Some(Tenant(s)) }
    }

    /// Returns the tenant string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The reason a string could not be read as a [`Tenant`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TenantError {
    /// The tenant value was the empty string.
    #[error("tenant must not be empty")]
    Empty,
}

impl FromStr for Tenant {
    type Err = TenantError;

    /// Parses a tenant, failing with [`TenantError::Empty`] when the string is empty.
    fn from_str(s: &str) -> Result<Tenant, Self::Err> {
        Tenant::new(s).ok_or(TenantError::Empty)
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant::{Tenant, TenantError};
    use std::str::FromStr;

    #[test]
    fn a_non_empty_value_becomes_a_tenant() {
        assert_eq!(Tenant::new("acme").map(|t| t.as_str().to_owned()), Some("acme".to_owned()));
    }

    #[test]
    fn an_empty_value_is_rejected() {
        assert_eq!(Tenant::new(""), None);
    }

    #[test]
    fn parsing_an_empty_string_reports_the_empty_error() {
        assert_eq!(Tenant::from_str(""), Err(TenantError::Empty));
    }

    #[test]
    fn a_tenant_displays_and_serializes_transparently() {
        let tenant = Tenant::from_str("acme").unwrap();

        assert_eq!(tenant.to_string(), "acme");
        assert_eq!(serde_json::to_string(&tenant).unwrap(), r#""acme""#);
        assert_eq!(serde_json::from_str::<Tenant>(r#""acme""#).unwrap(), tenant);
    }
}
