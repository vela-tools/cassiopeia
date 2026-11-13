use crate::version::StaVersion;
use derive_more::{Display, Into};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;
use url::{ParseError, Url};

/// The service root of a SensorThings API server: the URL every resource of the server is addressed
/// under, whose last path segment is the version it serves (OGC 18-088 section 9.1).
///
/// A service root is an `http` or `https` URL without user info, query or fragment, so it can be
/// written into a snapshot, a report or a log without carrying a credential. It is stored without a
/// trailing slash, so `https://iot.example.org/sta/v1.1/` and `https://iot.example.org/sta/v1.1` are
/// one root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, Into, Serialize, Deserialize)]
#[display("{url}")]
#[serde(try_from = "Url", into = "Url")]
pub struct ServiceRoot {
    /// The root URL, without a trailing slash.
    url: Url,
    /// The version the root's last path segment names.
    #[into(skip)]
    version: StaVersion,
}

/// Why a URL is no SensorThings service root.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ServiceRootError {
    /// The text is no absolute URL.
    #[error("a service root is an absolute `http` or `https` URL: {0}")]
    NotAUrl(ParseError),

    /// The URL's scheme is neither `http` nor `https`.
    #[error("a service root is an `http` or `https` URL, not a `{scheme}` one")]
    Scheme {
        /// The scheme the URL names.
        scheme: Box<str>,
    },

    /// The URL carries a user name or a password.
    #[error("a service root carries no user name or password")]
    Credentials,

    /// The URL carries a query.
    #[error("a service root carries no query")]
    Query,

    /// The URL carries a fragment.
    #[error("a service root carries no fragment")]
    Fragment,

    /// The URL's last path segment is no version.
    #[error("a service root ends with the version it serves, such as `/v1.1`")]
    MissingVersion,

    /// The URL's last path segment is a version Cassiopeia does not read.
    #[error("SensorThings API version `{0}` is not supported: Cassiopeia reads `v1.0` and `v1.1`")]
    UnsupportedVersion(Box<str>),
}

impl ServiceRoot {
    /// The root URL, without a trailing slash.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// The version the server serves under this root.
    #[must_use]
    pub const fn version(&self) -> StaVersion {
        self.version
    }

    /// The root's host, without its port.
    #[must_use]
    pub fn host(&self) -> Option<&str> {
        self.url.host_str()
    }
}

impl TryFrom<Url> for ServiceRoot {
    type Error = ServiceRootError;

    fn try_from(mut url: Url) -> Result<ServiceRoot, ServiceRootError> {
        match url.scheme() {
            "http" | "https" => {}
            other => return Err(ServiceRootError::Scheme { scheme: other.into() }),
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(ServiceRootError::Credentials);
        }
        if url.query().is_some() {
            return Err(ServiceRootError::Query);
        }
        if url.fragment().is_some() {
            return Err(ServiceRootError::Fragment);
        }
        // The trimmed path outlives the URL's borrow, since the URL is rewritten with it below.
        let path = url.path().trim_end_matches('/').to_owned();
        let segment = path.rsplit_once('/').map_or(path.as_str(), |(_, last)| last);
        let version = version_of(segment)?;
        url.set_path(&path);
        Ok(ServiceRoot { url, version })
    }
}

// A root is parsed as a URL and then checked as one, which no derive expresses.
impl FromStr for ServiceRoot {
    type Err = ServiceRootError;

    /// Reads a service root written as text, such as on the command line.
    fn from_str(text: &str) -> Result<ServiceRoot, ServiceRootError> {
        ServiceRoot::try_from(Url::parse(text).map_err(ServiceRootError::NotAUrl)?)
    }
}

/// The version a service root's last path segment names.
fn version_of(segment: &str) -> Result<StaVersion, ServiceRootError> {
    if !is_version(segment) {
        return Err(ServiceRootError::MissingVersion);
    }
    segment.parse().map_err(|_| ServiceRootError::UnsupportedVersion(segment.into()))
}

/// Whether a path segment is written as a version is, `"v" major "." minor` (OGC 18-088 section
/// 9.1), whichever version it names.
fn is_version(segment: &str) -> bool {
    let is_number = |digits: &str| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
    segment
        .strip_prefix('v')
        .and_then(|number| number.split_once('.'))
        .is_some_and(|(major, minor)| is_number(major) && is_number(minor))
}

#[cfg(test)]
mod tests {
    use crate::{
        service_root::{ServiceRoot, ServiceRootError},
        version::StaVersion,
    };
    use pretty_assertions::assert_eq;
    use url::{ParseError, Url};

    fn root(text: &str) -> Result<ServiceRoot, ServiceRootError> {
        ServiceRoot::try_from(Url::parse(text).unwrap())
    }

    #[test]
    fn a_service_root_reads_its_version() {
        let root = root("https://api-samenmeten.rivm.nl/v1.0").unwrap();

        assert_eq!(root.version(), StaVersion::V1_0);
        assert_eq!(root.to_string(), "https://api-samenmeten.rivm.nl/v1.0");
        assert_eq!(root.host(), Some("api-samenmeten.rivm.nl"));
    }

    #[test]
    fn a_trailing_slash_is_removed() {
        let root = root("https://iot.example.org/sta/v1.1/").unwrap();

        assert_eq!(root.to_string(), "https://iot.example.org/sta/v1.1");
        assert_eq!(root.version(), StaVersion::V1_1);
    }

    #[test]
    fn the_host_is_named_without_its_port() {
        assert_eq!(root("http://localhost:8080/FROST-Server/v1.1").unwrap().host(), Some("localhost"));
    }

    #[test]
    fn a_service_root_without_version_is_refused() {
        assert_eq!(root("https://h.example/api"), Err(ServiceRootError::MissingVersion));
        assert_eq!(root("https://h.example/"), Err(ServiceRootError::MissingVersion));
    }

    #[test]
    fn an_unsupported_version_is_refused() {
        assert_eq!(root("https://h.example/v2.0"), Err(ServiceRootError::UnsupportedVersion("v2.0".into())));
    }

    #[test]
    fn a_query_user_info_fragment_or_other_scheme_is_refused() {
        assert_eq!(root("https://h.example/v1.1?key=1"), Err(ServiceRootError::Query));
        assert_eq!(root("https://u:p@h.example/v1.1"), Err(ServiceRootError::Credentials));
        assert_eq!(root("https://h.example/v1.1#x"), Err(ServiceRootError::Fragment));
        assert_eq!(root("ftp://h.example/v1.1"), Err(ServiceRootError::Scheme { scheme: "ftp".into() }));
    }

    #[test]
    fn a_service_root_written_as_text_is_parsed_and_checked() {
        assert_eq!(
            "https://iot.example.org/sta/v1.1/".parse::<ServiceRoot>().unwrap(),
            root("https://iot.example.org/sta/v1.1").unwrap()
        );
        assert_eq!(
            "iot.example.org/v1.1".parse::<ServiceRoot>(),
            Err(ServiceRootError::NotAUrl(ParseError::RelativeUrlWithoutBase))
        );
        assert_eq!("https://h.example/api".parse::<ServiceRoot>(), Err(ServiceRootError::MissingVersion));
    }

    #[test]
    fn a_service_root_serializes_as_its_url_and_is_checked_when_read() {
        let root = root("https://h.example/v1.1/").unwrap();

        assert_eq!(serde_json::to_string(&root).unwrap(), r#""https://h.example/v1.1""#);
        assert_eq!(serde_json::from_str::<ServiceRoot>(r#""https://h.example/v1.1""#).unwrap(), root);
        assert!(serde_json::from_str::<ServiceRoot>(r#""https://h.example/api""#).is_err());
    }
}
