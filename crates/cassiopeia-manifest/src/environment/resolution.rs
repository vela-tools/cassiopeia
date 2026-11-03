use crate::environment::{error::EnvironmentError, field::TemplatedField};
use cassiopeia_common::{
    environment::{
        registry::register,
        template::{Resolved, Template, TemplateError},
        variable_name::VariableName,
    },
    input::Input,
    masked_url::MaskedUrl,
    request_header::RequestHeader,
};
use http::HeaderName;
use secrecy::{ExposeSecret, SecretString};
use std::str::FromStr;
use url::Url;

/// Fills the variables of `text`, the content of `field`, with the values `lookup` gives, and records
/// every value read as a secret every output sink masks from then on.
fn resolve(
    text: &str,
    field: &TemplatedField,
    lookup: impl Fn(&VariableName) -> Result<SecretString, TemplateError>,
) -> Result<(Template, Resolved), EnvironmentError> {
    let template_error = |reason| EnvironmentError::Template {
        // The error owns the name of the field, while the caller keeps it for later failures.
        field: field.clone(),
        reason,
    };
    let template = Template::from_str(text).map_err(template_error)?;
    let resolved = template.resolve(lookup).map_err(template_error)?;
    register(resolved.secrets());
    Ok((template, resolved))
}

/// Reads an input's `source`, filling its variables from `lookup`.
///
/// A source that reads the environment must be a URL: a local path has no masked form, so a value
/// read into one could be shown by any message that names the file.
///
/// # Errors
/// Returns [`EnvironmentError::Template`] for a malformed template or an unreadable variable,
/// [`EnvironmentError::LocalSource`] when a source that reads the environment names a local file,
/// and [`EnvironmentError::Source`] when a source without variables is an unusable file URL.
pub fn read_source(text: &str, lookup: impl Fn(&VariableName) -> Result<SecretString, TemplateError>) -> Result<Input, EnvironmentError> {
    let (template, resolved) = resolve(text, &TemplatedField::InputSource, lookup)?;
    if !template.reads_environment() {
        return Ok(Input::from_str(resolved.text().expose_secret())?);
    }
    // The parse error of a source built from a secret quotes it, so it is replaced, not passed on.
    match Input::from_str(resolved.text().expose_secret()) {
        Ok(Input::Remote(url)) => Ok(Input::Remote(MaskedUrl::with_secrets(url.into_url(), resolved.into_secrets()))),
        Ok(Input::Local(_)) | Err(_) => Err(EnvironmentError::LocalSource),
    }
}

/// Reads the output's broker `url`, filling its variables from `lookup`.
///
/// # Errors
/// Returns [`EnvironmentError::Template`] for a malformed template or an unreadable variable, and
/// [`EnvironmentError::InvalidUrl`] when the filled-in text is not a URL.
pub fn read_url(text: &str, lookup: impl Fn(&VariableName) -> Result<SecretString, TemplateError>) -> Result<MaskedUrl, EnvironmentError> {
    let field = TemplatedField::OutputUrl;
    let (_, resolved) = resolve(text, &field, lookup)?;
    let url = Url::parse(resolved.text().expose_secret()).map_err(|reason| EnvironmentError::InvalidUrl { field, reason })?;
    Ok(MaskedUrl::with_secrets(url, resolved.into_secrets()))
}

/// Reads one header, filling the variables of its value from `lookup`. `field` names the header's
/// place in the manifest, given its validated name.
///
/// # Errors
/// Returns [`EnvironmentError::Header`] when the name or the filled-in value is not valid HTTP, and
/// [`EnvironmentError::Template`] for a malformed template or an unreadable variable.
pub fn read_header(
    name: &str,
    value: &str,
    field: fn(HeaderName) -> TemplatedField,
    lookup: impl Fn(&VariableName) -> Result<SecretString, TemplateError>,
) -> Result<RequestHeader, EnvironmentError> {
    let header_name = RequestHeader::parse_name(name)?;
    let (_, resolved) = resolve(value, &field(header_name), lookup)?;
    let header = RequestHeader::new(name, resolved.text().expose_secret())?;
    Ok(header.with_secrets(resolved.into_secrets()))
}

#[cfg(test)]
mod tests {
    use crate::environment::{
        error::EnvironmentError,
        field::TemplatedField,
        resolution::{read_header, read_source, read_url},
    };
    use cassiopeia_common::{
        environment::{registry::mask, template::TemplateError, variable_name::VariableName},
        input::Input,
    };
    use pretty_assertions::assert_eq;
    use secrecy::SecretString;
    use std::path::PathBuf;

    /// A lookup over a fixed table, standing in for the process environment.
    fn table(entries: &'static [(&'static str, &'static str)]) -> impl Fn(&VariableName) -> Result<SecretString, TemplateError> {
        move |name| {
            entries
                .iter()
                .find(|(key, _)| *key == name.as_str())
                .map(|(_, value)| SecretString::from(*value))
                .ok_or_else(|| TemplateError::Unset { name: name.clone() })
        }
    }

    #[test]
    fn a_source_url_reads_its_key_and_shows_it_masked() {
        let input = read_source("https://api.example/records?apikey=${API_KEY}", table(&[("API_KEY", "src-key-91c")])).unwrap();

        let Input::Remote(url) = &input else {
            panic!("expected a remote source");
        };
        assert_eq!(url.url().as_str(), "https://api.example/records?apikey=src-key-91c");
        assert_eq!(input.to_string(), "https://api.example/records?apikey=${API_KEY}");
        assert!(!format!("{input:?}").contains("src-key-91c"));
    }

    #[test]
    fn a_read_source_key_is_masked_by_every_output_sink() {
        read_source("https://api.example/?k=${SINK_KEY}", table(&[("SINK_KEY", "sink-key-4d2")])).unwrap();

        assert_eq!(
            mask("GET https://api.example/?k=sink-key-4d2 failed"),
            "GET https://api.example/?k=${SINK_KEY} failed"
        );
    }

    #[test]
    fn a_source_without_variables_reads_as_written() {
        assert_eq!(
            read_source("data/stations.csv", table(&[])).unwrap(),
            Input::Local(PathBuf::from("data/stations.csv"))
        );
    }

    #[test]
    fn an_escaped_opening_in_a_source_is_a_literal() {
        let input = read_source("https://api.example/?q=$${x}", table(&[])).unwrap();

        let Input::Remote(url) = input else {
            panic!("expected a remote source");
        };
        assert_eq!(url.url().query(), Some("q=${x}"));
    }

    #[test]
    fn a_local_source_that_reads_the_environment_is_refused() {
        let error = read_source("${DATA_DIR}/stations.csv", table(&[("DATA_DIR", "/srv/data")])).unwrap_err();

        assert!(matches!(error, EnvironmentError::LocalSource));
        assert!(!error.to_string().contains("/srv/data"));
    }

    #[test]
    fn an_unset_source_variable_names_the_variable_and_the_field() {
        let error = read_source("https://api.example/?apikey=${MISSING_KEY}", table(&[])).unwrap_err();

        assert_eq!(error.to_string(), "an input's 'source': the environment variable 'MISSING_KEY' is not set");
    }

    #[test]
    fn an_output_url_reads_its_host() {
        let url = read_url("https://${BROKER_HOST}/", table(&[("BROKER_HOST", "broker.internal")])).unwrap();

        assert_eq!(url.url().as_str(), "https://broker.internal/");
        assert_eq!(url.to_string(), "https://${BROKER_HOST}/");
    }

    #[test]
    fn an_output_url_that_is_no_url_once_filled_is_refused_without_the_value() {
        let error = read_url("${BROKER}", table(&[("BROKER", "not a url")])).unwrap_err();

        assert!(matches!(
            error,
            EnvironmentError::InvalidUrl {
                field: TemplatedField::OutputUrl,
                ..
            }
        ));
        assert!(!error.to_string().contains("not a url"));
    }

    #[test]
    fn a_header_reads_its_value_and_keeps_its_template() {
        let header = read_header(
            "Authorization",
            "Bearer ${BROKER_TOKEN}",
            TemplatedField::OutputHeader,
            table(&[("BROKER_TOKEN", "tok-55e")]),
        )
        .unwrap();

        assert_eq!(header.value().to_str().unwrap(), "Bearer tok-55e");
    }

    #[test]
    fn an_unset_header_variable_names_the_header() {
        let error = read_header("X-Api-Key", "${MISSING_HEADER_KEY}", TemplatedField::InputHeader, table(&[])).unwrap_err();

        assert_eq!(
            error.to_string(),
            "the input header 'x-api-key': the environment variable 'MISSING_HEADER_KEY' is not set"
        );
    }

    #[test]
    fn a_header_value_that_is_invalid_once_filled_is_refused_without_the_value() {
        let error = read_header("X-Api-Key", "${BAD}", TemplatedField::InputHeader, table(&[("BAD", "line\nbreak")])).unwrap_err();

        assert!(matches!(error, EnvironmentError::Header(_)));
        assert!(!error.to_string().contains("line"));
    }
}
