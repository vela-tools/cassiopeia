use crate::environment::secret::{MaskStyle, Secrets};
use derive_more::{Debug, Display};
use serde::{Serialize, Serializer};
use std::borrow::Cow;
use url::{ParseError, Url};

/// A URL that shows itself with every value it read from the environment masked as `${NAME}`.
///
/// The URL a request goes to is reached through [`MaskedUrl::url`]; `Display`, `Debug`, and
/// serialization only ever show the masked form, so a key carried in a query, a path, or a password
/// stays out of every message that names the URL. A URL that read nothing from the environment
/// shows itself unchanged.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Display)]
#[display("{}", secrets.mask(url.as_str(), MaskStyle::Plain))]
#[debug("{}", secrets.mask(url.as_str(), MaskStyle::Plain))]
pub struct MaskedUrl {
    /// The URL a request goes to.
    url: Url,
    /// The values read from the environment into the URL.
    secrets: Secrets,
}

impl MaskedUrl {
    /// Wraps a URL that read nothing from the environment.
    #[must_use]
    pub const fn plain(url: Url) -> MaskedUrl {
        MaskedUrl { url, secrets: Secrets::new() }
    }

    /// Wraps a URL built from the values in `secrets`.
    #[must_use]
    pub const fn with_secrets(url: Url, secrets: Secrets) -> MaskedUrl {
        MaskedUrl { url, secrets }
    }

    /// Returns the URL a request goes to, secrets and all. Only a network call may use it.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// Consumes the wrapper, returning the URL a request goes to.
    #[must_use]
    pub fn into_url(self) -> Url {
        self.url
    }

    /// Returns the values read from the environment into the URL.
    #[must_use]
    pub const fn secrets(&self) -> &Secrets {
        &self.secrets
    }

    /// Writes the URL as the template it reads back from: every secret as `${NAME}`, and every
    /// literal `${` escaped as `$${`.
    #[must_use]
    pub fn template(&self) -> Cow<'_, str> {
        self.secrets.mask(self.url.as_str(), MaskStyle::Template)
    }

    /// Wraps `url`, which this URL led to, masking the same secrets: a redirect or a joined path
    /// can carry them along.
    #[must_use]
    pub fn led_to(&self, url: Url) -> MaskedUrl {
        MaskedUrl {
            url,
            // The derived URL masks what this one does, while this one keeps its own set.
            secrets: self.secrets.clone(),
        }
    }

    /// Resolves `path` against this URL, as [`Url::join`] does, masking the same secrets.
    ///
    /// # Errors
    /// Returns the [`ParseError`] [`Url::join`] reports.
    pub fn join(&self, path: &str) -> Result<MaskedUrl, ParseError> {
        Ok(self.led_to(self.url.join(path)?))
    }

    /// Replaces the query, as [`Url::set_query`] does.
    #[must_use]
    pub fn with_query(mut self, query: Option<&str>) -> MaskedUrl {
        self.url.set_query(query);
        self
    }
}

// Manual because the wire form is the template string, never the URL it resolves to.
impl Serialize for MaskedUrl {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.template())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        environment::{
            secret::{Secret, Secrets},
            variable_name::VariableName,
        },
        masked_url::MaskedUrl,
    };
    use pretty_assertions::assert_eq;
    use secrecy::SecretString;
    use std::str::FromStr;
    use url::Url;

    fn keyed() -> MaskedUrl {
        let mut secrets = Secrets::new();
        secrets.insert(Secret::new(VariableName::from_str("API_KEY").unwrap(), &SecretString::from("s3cr3t")));
        MaskedUrl::with_secrets(Url::parse("https://api.example/v1/?apikey=s3cr3t").unwrap(), secrets)
    }

    #[test]
    fn display_and_debug_mask_the_secret() {
        let url = keyed();

        assert_eq!(url.to_string(), "https://api.example/v1/?apikey=${API_KEY}");
        assert_eq!(format!("{url:?}"), "https://api.example/v1/?apikey=${API_KEY}");
    }

    #[test]
    fn the_request_url_keeps_the_secret() {
        assert_eq!(keyed().url().as_str(), "https://api.example/v1/?apikey=s3cr3t");
    }

    #[test]
    fn serialization_writes_the_template() {
        assert_eq!(serde_json::to_string(&keyed()).unwrap(), r#""https://api.example/v1/?apikey=${API_KEY}""#);
    }

    #[test]
    fn serialization_escapes_a_literal_placeholder_opening() {
        let url = MaskedUrl::plain(Url::parse("https://api.example/?q=${x}").unwrap());

        assert_eq!(serde_json::to_string(&url).unwrap(), r#""https://api.example/?q=$${x}""#);
    }

    #[test]
    fn a_joined_url_masks_the_same_secret() {
        let mut secrets = Secrets::new();
        secrets.insert(Secret::new(VariableName::from_str("TOKEN").unwrap(), &SecretString::from("t0k3n")));
        let base = MaskedUrl::with_secrets(Url::parse("https://broker.example/t0k3n/").unwrap(), secrets);

        let endpoint = base.join("ngsi-ld/v1/entityOperations/upsert").unwrap().with_query(Some("options=update"));

        assert_eq!(
            endpoint.url().as_str(),
            "https://broker.example/t0k3n/ngsi-ld/v1/entityOperations/upsert?options=update"
        );
        assert_eq!(
            endpoint.to_string(),
            "https://broker.example/${TOKEN}/ngsi-ld/v1/entityOperations/upsert?options=update"
        );
    }

    #[test]
    fn a_url_led_to_masks_the_same_secret() {
        let redirected = keyed().led_to(Url::parse("https://mirror.example/?apikey=s3cr3t").unwrap());

        assert_eq!(redirected.to_string(), "https://mirror.example/?apikey=${API_KEY}");
    }

    #[test]
    fn a_plain_url_shows_itself_unchanged() {
        let url = MaskedUrl::plain(Url::parse("https://example.org/a.json").unwrap());

        assert_eq!(url.to_string(), "https://example.org/a.json");
    }
}
