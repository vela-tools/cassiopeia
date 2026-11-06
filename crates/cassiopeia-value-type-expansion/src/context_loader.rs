use crate::{context_load_error::ContextLoadError, core_context::CORE_CONTEXT};
use cassiopeia_common::user_agent::UserAgent;
use dashmap::DashMap;
use iri_rs::{Iri, IriBuf};
use jsonld::{LoadError, Loader, RemoteDocument};
use reqwest::{
    blocking::Client,
    header::{ACCEPT, HeaderValue},
};
use serde_json::Value as JsonValue;
use std::{future::ready, time::Duration};

/// How long one remote `@context` may take to load, from connecting to the end of its body.
const LOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// The media types a `@context` is asked for in, JSON-LD first (JSON-LD 1.1 clause 9.1).
const ACCEPTED: HeaderValue = HeaderValue::from_static("application/ld+json, application/json;q=0.9, */*;q=0.1");

/// Loads the `@context` documents an expansion needs: the bundled core `@context`, and every other one
/// over HTTP, once per run.
///
/// A fetched document is read as JSON whatever media type it is served as. JSON-LD 1.1 expects
/// `application/ld+json` or another JSON type, but Smart Data Models publishes every data model's
/// `@context` from `raw.githubusercontent.com`, which serves it as `text/plain`, and those are the
/// contexts a run attaches by default.
pub struct ContextLoader {
    /// Every document loaded so far, keyed by the IRI it was asked for under, the core `@context`
    /// among them from the start, so a `@context` several mappings share is fetched once.
    ///
    /// A `@context` IRI compares as a string, as every IRI does in JSON-LD and RDF (RDF 1.1 Concepts
    /// clause 3.2), so two spellings of one IRI are two cache entries, as they are two documents to a
    /// JSON-LD processor.
    documents: DashMap<IriBuf, RemoteDocument>,
    /// The HTTP client every remote `@context` is fetched through.
    client: Client,
}

impl ContextLoader {
    /// Builds a loader holding the bundled core `@context`, fetching every other one under
    /// `user_agent`.
    ///
    /// # Errors
    /// Returns [`ContextLoadError::Client`] when the HTTP client cannot be built.
    pub fn new(user_agent: &UserAgent, core: RemoteDocument) -> Result<ContextLoader, ContextLoadError> {
        let client = Client::builder()
            .user_agent(user_agent.as_str())
            .timeout(LOAD_TIMEOUT)
            .build()
            .map_err(ContextLoadError::Client)?;
        let documents = DashMap::new();
        documents.insert(CORE_CONTEXT.into(), core);

        Ok(ContextLoader { documents, client })
    }

    /// The document at `url`: the one loaded before, or else one fetched now and kept for later.
    fn cached_or_fetched(&self, url: Iri<&str>) -> Result<RemoteDocument, ContextLoadError> {
        if let Some(document) = self.documents.get(url.as_str()) {
            // The expansion takes the document by value, and the cache keeps its own copy for the
            // next mapping that names the same `@context`.
            return Ok(document.clone());
        }

        let document = self.fetch(url)?;
        // As above: the cache keeps a copy of what the expansion takes.
        self.documents.insert(IriBuf::from(url), document.clone());

        Ok(document)
    }

    /// Fetches one remote `@context` document.
    fn fetch(&self, url: Iri<&str>) -> Result<RemoteDocument, ContextLoadError> {
        let response = self
            .client
            .get(url.as_str())
            .header(ACCEPT, ACCEPTED)
            .send()
            .map_err(ContextLoadError::Request)?;
        let status = response.status();
        if !status.is_success() {
            return Err(ContextLoadError::Status { status });
        }
        let document: JsonValue = response.json().map_err(ContextLoadError::Body)?;

        Ok(RemoteDocument::from_serde_json(Some(url.into()), None, document))
    }
}

impl Loader for ContextLoader {
    type Error = ContextLoadError;

    // The fetch blocks rather than awaits, so the document is ready by the time the future is
    // returned: an expansion is driven to completion on the calling thread, which has nothing else to
    // run while it waits.
    fn load(&self, url: Iri<&str>) -> impl Future<Output = Result<RemoteDocument, LoadError<ContextLoadError>>> + Send {
        ready(self.cached_or_fetched(url).map_err(|source| LoadError::new(url.into(), source)))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_load_error::ContextLoadError,
        context_loader::ContextLoader,
        core_context::{CORE_CONTEXT, core_context_document},
    };
    use cassiopeia_common::user_agent::UserAgent;
    use futures_executor::block_on;
    use iri_rs::IriBuf;
    use jsonld::Loader;
    use pretty_assertions::assert_eq;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn loader() -> ContextLoader {
        ContextLoader::new(&UserAgent::from("test".to_owned()), core_context_document().unwrap()).unwrap()
    }

    /// Serves one response, then stops listening, and returns the URL it is served at.
    fn serve_once(status_line: &'static str, content_type: &'static str, body: &'static str) -> IriBuf {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let response = format!(
                "{status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        IriBuf::new(format!("http://{address}/context.jsonld")).unwrap()
    }

    #[test]
    fn the_core_context_is_served_without_a_request() {
        let document = block_on(loader().load(CORE_CONTEXT)).unwrap();

        assert_eq!(document.url().map(IriBuf::as_str), Some(CORE_CONTEXT.as_str()));
    }

    #[test]
    fn a_context_served_as_plain_text_is_read_as_json_as_smart_data_models_serves_it() {
        let url = serve_once(
            "HTTP/1.1 200 OK",
            "text/plain; charset=utf-8",
            r#"{"@context": {"PostalAddress": "http://example.org/PostalAddress"}}"#,
        );

        let document = block_on(loader().load(url.as_ref())).unwrap().into_serde_json();

        assert_eq!(document.document()["@context"]["PostalAddress"], "http://example.org/PostalAddress");
    }

    #[test]
    fn a_context_loaded_once_is_served_again_without_a_second_request() {
        let url = serve_once("HTTP/1.1 200 OK", "application/ld+json", r#"{"@context": {}}"#);
        let loader = loader();

        assert!(block_on(loader.load(url.as_ref())).is_ok());
        // The server answered its one request and stopped listening, so this load reads the cache.
        assert!(block_on(loader.load(url.as_ref())).is_ok());
    }

    #[test]
    fn a_context_the_server_does_not_have_fails_naming_the_status_and_the_url() {
        let url = serve_once("HTTP/1.1 404 Not Found", "text/plain", "missing");

        let error = block_on(loader().load(url.as_ref())).unwrap_err();

        assert_eq!(error.target, url);
        assert!(matches!(error.source, ContextLoadError::Status { status } if status.as_u16() == 404));
    }

    #[test]
    fn a_context_that_is_not_json_fails_as_an_unreadable_body() {
        let url = serve_once("HTTP/1.1 200 OK", "application/ld+json", "not json");

        let error = block_on(loader().load(url.as_ref())).unwrap_err();

        assert!(matches!(error.source, ContextLoadError::Body(_)));
    }
}
