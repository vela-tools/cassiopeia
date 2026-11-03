use crate::{
    downloaded_file::DownloadedFile,
    error::{CollectorError, ERROR_BODY_CAP},
};
use cassiopeia_common::{
    captured_body::CapturedBody,
    http_retry::{
        backoff::{BackoffSleep, DEFAULT_MAX_ATTEMPTS, retry_delay, sleep_or_shutdown},
        retry_after::RetryAfter,
        transient_status::is_transient,
    },
    masked_url::MaskedUrl,
    request_header::RequestHeaders,
    user_agent::UserAgent,
};
use http::StatusCode;
use mediatype::MediaTypeBuf;
use reqwest::{blocking::Client, header::CONTENT_TYPE};
use std::{fs::File, io::Write, path::Path, sync::atomic::AtomicBool};
use tracing::debug;

/// Fetches remote sources over HTTP, announcing every request with one fixed `User-Agent` and
/// attaching the same user-supplied headers to each.
///
/// The HTTP client is built on the first download rather than at construction: a blocking reqwest
/// client eagerly spawns its own runtime thread, and a run whose sources are all local files never
/// reaches the network.
pub struct Downloader {
    /// The identifier every request announces itself with.
    user_agent: UserAgent,
    /// The headers attached to every request, typically carrying the source's API key.
    headers: RequestHeaders,
    /// The process-wide shutdown flag, which ends a wait between attempts.
    shutdown: &'static AtomicBool,
    /// The blocking HTTP client, built on the first download and reused by every later one.
    client: Option<Client>,
}

/// One answer a server gave to a download request, read in full.
struct Answer {
    /// The status the server answered with.
    status: StatusCode,
    /// The URL the answer was served from after any redirects, masked as the requested one is.
    served_from: MaskedUrl,
    /// The media type the server declared, when it declared one that parses.
    content_type: Option<MediaTypeBuf>,
    /// The least wait the server asked for before the next request.
    retry_after: Option<RetryAfter>,
    /// The body: the resource on a success, the server's explanation otherwise.
    body: CapturedBody,
}

impl Downloader {
    /// Creates a downloader that identifies itself as `user_agent`, attaches `headers` to every
    /// request, stops waiting to retry once `shutdown` is set, and has not yet touched the network.
    #[must_use]
    pub const fn new(user_agent: UserAgent, headers: RequestHeaders, shutdown: &'static AtomicBool) -> Downloader {
        Downloader {
            user_agent,
            headers,
            shutdown,
            client: None,
        }
    }

    /// Downloads a single file from `url` to `dest`, returning that path with the URL it was served
    /// from after any redirects and the media type the server declared for it.
    ///
    /// A server that answers `429` or a `5xx` is asked again with backoff, waiting at least the
    /// `Retry-After` it sent, up to [`DEFAULT_MAX_ATTEMPTS`] attempts in all; its last answer is the
    /// failure once they are spent, or once shutdown cuts a wait short. Any other non-success status
    /// fails at once, since sending the same request again cannot change it.
    ///
    /// Every URL a failure names is masked as `url` is, and a transport failure is stripped of the
    /// URL reqwest would quote, which is the request URL secrets and all.
    ///
    /// # Errors
    ///
    /// Returns [`CollectorError::ClientInit`] when the HTTP client cannot be built,
    /// [`CollectorError::Http`] when the request never completes, [`CollectorError::HttpStatus`]
    /// when the server answers with a non-success status, and [`CollectorError::Io`] when the
    /// response body cannot be written to `dest`.
    pub fn download_to_file(&mut self, url: &MaskedUrl, dest: &Path) -> Result<DownloadedFile, CollectorError> {
        let mut attempt: u32 = 1;
        let answer = loop {
            let answer = self.ask(url)?;
            if !is_transient(answer.status) || attempt >= DEFAULT_MAX_ATTEMPTS {
                break answer;
            }
            let delay = retry_delay(attempt, answer.retry_after);
            debug!(
                "{url} answered {}; retrying in {delay:?} (attempt {attempt} of {DEFAULT_MAX_ATTEMPTS})",
                answer.status
            );
            if sleep_or_shutdown(delay, self.shutdown) == BackoffSleep::ShutdownRequested {
                break answer;
            }
            attempt += 1;
        };

        // A 4xx/5xx answers with an error page, not the resource; treating that body as the download
        // would silently write garbage in place of the file, so a non-success status is a failure.
        if !answer.status.is_success() {
            return Err(CollectorError::HttpStatus {
                // The error owns the URL, while the caller keeps the one it asked for.
                url: Box::new(url.clone()),
                status: answer.status,
                body: Box::new(answer.body.echo(ERROR_BODY_CAP)),
            });
        }
        let body = answer.body.bytes();
        let mut file = File::create(dest).map_err(|source| CollectorError::Io {
            source,
            path: dest.to_path_buf(),
        })?;
        file.write_all(body).map_err(|source| CollectorError::Io {
            source,
            path: dest.to_path_buf(),
        })?;
        file.flush().map_err(|source| CollectorError::Io {
            source,
            path: dest.to_path_buf(),
        })?;
        Ok(DownloadedFile::new(dest.to_path_buf(), answer.served_from, answer.content_type))
    }

    /// Sends one request for `url` and reads the whole answer, whatever its status.
    ///
    /// The status is checked by the caller rather than through `error_for_status`, which consumes the
    /// response and throws the body away, and that body is the server's only explanation of a refusal.
    fn ask(&mut self, url: &MaskedUrl) -> Result<Answer, CollectorError> {
        let response = self.client()?.get(url.url().as_str()).send().map_err(|source| CollectorError::Http {
            source: source.without_url(),
            // The error owns the URL after the borrowed request is dropped.
            url: Box::new(url.clone()),
        })?;

        let status = response.status();
        // The body consumes the response, while the download keeps naming where it was served from.
        let served_from = url.led_to(response.url().clone());
        // An unparseable `Content-Type` declares nothing a reader could rely on, so it is dropped.
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<MediaTypeBuf>().ok());
        let retry_after = RetryAfter::from_headers(response.headers());
        let body = match response.bytes() {
            Ok(bytes) => CapturedBody::capped(bytes.to_vec(), ERROR_BODY_CAP.max(bytes.len())),
            Err(source) => {
                if status.is_success() {
                    return Err(CollectorError::Http {
                        source: source.without_url(),
                        // The error owns the URL, while the caller keeps the one it asked for.
                        url: Box::new(url.clone()),
                    });
                }
                CapturedBody::unreadable(&source.without_url())
            }
        };
        Ok(Answer {
            status,
            served_from,
            content_type,
            retry_after,
            body,
        })
    }

    /// The HTTP client, built with this downloader's `User-Agent` and headers on first use and reused
    /// after.
    fn client(&mut self) -> Result<&Client, CollectorError> {
        let client = match self.client.take() {
            Some(client) => client,
            // Another workspace crate enables reqwest's `gzip` feature, which feature unification turns
            // on for this client as well. Some servers (the City of Vienna WFS among them) interleave a
            // gzip-encoded HTTP/2 body with empty DATA frames. h2 accepts at most 100 empty non-final
            // DATA frames over a connection's lifetime, a fixed limit nothing above h2 can raise, and
            // closes the connection with `too_many_data_frames` past it (hyperium/h2#961). The same
            // servers send an uncompressed body in full frames, so the download asks for that instead.
            None => Client::builder()
                .user_agent(self.user_agent.as_str())
                .default_headers(self.headers.sensitive().collect())
                .no_gzip()
                .build()
                .map_err(|source| CollectorError::ClientInit { source })?,
        };

        Ok(self.client.insert(client))
    }
}

#[cfg(test)]
mod tests {
    use crate::{downloader::Downloader, error::CollectorError};
    use cassiopeia_common::{
        environment::{
            secret::{Secret, Secrets},
            variable_name::VariableName,
        },
        masked_url::MaskedUrl,
        request_header::{RequestHeader, RequestHeaders},
        user_agent::UserAgent,
    };
    use pretty_assertions::assert_eq;
    use secrecy::SecretString;
    use std::{
        error::Error,
        fs::read_to_string,
        io::{Read as _, Write as _},
        net::TcpListener,
        str::FromStr,
        sync::{
            atomic::AtomicBool,
            mpsc::{Receiver, channel},
        },
        thread,
        time::{Duration, Instant},
    };
    use tempfile::TempDir;
    use url::Url;

    /// Serves exactly one HTTP response with the given status line and body, then closes. The
    /// receiver hands back the raw request the server read, so a test can assert on its headers.
    fn serve_once(status_line: &'static str, body: &'static str) -> (MaskedUrl, Receiver<String>) {
        serve_once_with_headers(status_line, "", body)
    }

    /// Serves one response like [`serve_once`], with `headers` (each line ending in `\r\n`) added.
    fn serve_once_with_headers(status_line: &'static str, headers: impl Into<String>, body: &'static str) -> (MaskedUrl, Receiver<String>) {
        let headers = headers.into();
        let listener = TcpListener::bind("127.0.0.1:0").expect("a bindable port");
        let port = listener.local_addr().expect("a local address").port();
        let (request_tx, request_rx) = channel();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0_u8; 1024];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let _ = request_tx.send(String::from_utf8_lossy(&buffer[..read]).into_owned());
                let response = format!(
                    "HTTP/1.1 {status_line}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (
            MaskedUrl::plain(Url::parse(&format!("http://127.0.0.1:{port}/Sensor.json")).expect("a valid URL")),
            request_rx,
        )
    }

    /// Wraps `url`, which carries `value`, as a URL that read `value` from the variable `name`.
    fn reading(url: &str, name: &str, value: &str) -> MaskedUrl {
        let mut secrets = Secrets::new();
        secrets.insert(Secret::new(VariableName::from_str(name).expect("a valid name"), &SecretString::from(value)));
        MaskedUrl::with_secrets(Url::parse(url).expect("a valid URL"), secrets)
    }

    /// Renders an error and its whole cause chain, the way a diagnostic would show it.
    fn rendered(error: &dyn Error) -> String {
        let mut text = error.to_string();
        let mut cause = error.source();
        while let Some(link) = cause {
            text.push_str(" / ");
            text.push_str(&link.to_string());
            cause = link.source();
        }
        text
    }

    /// A shutdown flag never set, so every wait between attempts runs to its end.
    static RUNNING: AtomicBool = AtomicBool::new(false);

    /// A shutdown flag already set, so every wait between attempts ends at once.
    static STOPPING: AtomicBool = AtomicBool::new(true);

    fn downloader() -> Downloader {
        Downloader::new(UserAgent::from("cassiopeia/test".to_owned()), RequestHeaders::default(), &RUNNING)
    }

    fn downloader_with_headers(headers: Vec<RequestHeader>) -> Downloader {
        Downloader::new(UserAgent::from("cassiopeia/test".to_owned()), RequestHeaders::new(headers), &RUNNING)
    }

    /// The URL of the one resource a mock server serves in these tests.
    fn feed(server: &mockito::ServerGuard) -> MaskedUrl {
        MaskedUrl::plain(Url::parse(&format!("{}/feed.csv", server.url())).expect("a valid URL"))
    }

    #[test]
    fn a_refused_download_keeps_the_servers_own_explanation() {
        let (url, _requests) = serve_once("404 Not Found", "no schema published under that name");
        let directory = TempDir::new().expect("a temporary directory");

        let error = downloader()
            .download_to_file(&url, &directory.path().join("Sensor.json"))
            .expect_err("a refusal");

        let CollectorError::HttpStatus { url: named, status, body } = error else {
            panic!("expected a status failure");
        };
        assert_eq!(*named, url);
        assert_eq!(status.as_u16(), 404);
        assert_eq!(body.text(), "no schema published under that name");
    }

    #[test]
    fn a_refused_download_names_its_url_with_the_secret_masked() {
        let (url, _requests) = serve_once("401 Unauthorized", "bad key");
        let url = reading(&format!("{url}?apikey=refused-key-0a1"), "SOURCE_KEY", "refused-key-0a1");
        let directory = TempDir::new().expect("a temporary directory");

        let error = downloader()
            .download_to_file(&url, &directory.path().join("Sensor.json"))
            .expect_err("a refusal");

        let text = rendered(&error);
        assert!(text.contains("apikey=${SOURCE_KEY}"), "{text}");
        assert!(!text.contains("refused-key-0a1"), "{text}");
    }

    #[test]
    fn a_transport_failure_never_quotes_the_secret_its_url_carries() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a bindable port");
        let port = listener.local_addr().expect("a local address").port();
        drop(listener);
        let url = reading(&format!("http://127.0.0.1:{port}/feed?apikey=closed-key-7e2"), "SOURCE_KEY", "closed-key-7e2");
        let directory = TempDir::new().expect("a temporary directory");

        let error = downloader()
            .download_to_file(&url, &directory.path().join("feed.json"))
            .expect_err("a transport failure");

        let text = rendered(&error);
        assert!(matches!(error, CollectorError::Http { .. }), "{text}");
        assert!(!text.contains("closed-key-7e2"), "{text}");
    }

    #[test]
    fn a_redirected_download_masks_the_secret_on_the_url_it_was_served_from() {
        let (target, _target_requests) = serve_once("200 OK", "id;name\n");
        let (requested, _requests) = serve_once_with_headers("302 Found", format!("Location: {}?apikey=moved-key-3b9\r\n", target.url()), "");
        let requested = reading(&format!("{requested}?apikey=moved-key-3b9"), "SOURCE_KEY", "moved-key-3b9");
        let directory = TempDir::new().expect("a temporary directory");

        let downloaded = downloader()
            .download_to_file(&requested, &directory.path().join("feed.csv"))
            .expect("a download");

        assert_eq!(downloaded.url().to_string(), format!("{target}?apikey=${{SOURCE_KEY}}"));
    }

    #[test]
    fn a_refused_download_writes_no_file() {
        // A 4xx answers with an error page, not the resource; writing it would leave garbage behind.
        let (url, _requests) = serve_once("404 Not Found", "no such feed");
        let directory = TempDir::new().expect("a temporary directory");
        let destination = directory.path().join("Sensor.json");

        let _ = downloader().download_to_file(&url, &destination).expect_err("a refusal");

        assert!(!destination.exists());
    }

    #[test]
    fn a_successful_download_writes_the_body() {
        let (url, _requests) = serve_once("200 OK", r#"{"type":"object"}"#);
        let directory = TempDir::new().expect("a temporary directory");
        let destination = directory.path().join("Sensor.json");

        let downloaded = downloader().download_to_file(&url, &destination).expect("a download");

        assert_eq!(read_to_string(downloaded.path()).expect("the written file"), r#"{"type":"object"}"#);
    }

    #[test]
    fn a_download_announces_the_user_agent_it_was_built_with() {
        let (url, requests) = serve_once("200 OK", "{}");
        let directory = TempDir::new().expect("a temporary directory");

        downloader().download_to_file(&url, &directory.path().join("Sensor.json")).expect("a download");

        let request = requests.recv().expect("the served request");
        assert!(request.contains("user-agent: cassiopeia/test"), "request was: {request}");
    }

    #[test]
    fn a_download_sends_every_configured_header() {
        let (url, requests) = serve_once("200 OK", "{}");
        let directory = TempDir::new().expect("a temporary directory");
        let mut downloader = downloader_with_headers(vec![
            RequestHeader::new("Authorization", "Apikey replace-me").expect("a valid header"),
            RequestHeader::new("X-Client-Name", "cassiopeia-test").expect("a valid header"),
        ]);

        downloader.download_to_file(&url, &directory.path().join("Sensor.json")).expect("a download");

        let request = requests.recv().expect("the served request");
        assert!(request.contains("authorization: Apikey replace-me\r\n"), "request was: {request}");
        assert!(request.contains("x-client-name: cassiopeia-test\r\n"), "request was: {request}");
    }

    #[test]
    fn a_reused_downloader_sends_the_headers_on_every_request() {
        let (first, first_requests) = serve_once("200 OK", "{}");
        let (second, second_requests) = serve_once("200 OK", "{}");
        let directory = TempDir::new().expect("a temporary directory");
        let mut downloader = downloader_with_headers(vec![RequestHeader::new("X-Api-Key", "replace-me").expect("a valid header")]);

        downloader.download_to_file(&first, &directory.path().join("a.json")).expect("a download");
        downloader.download_to_file(&second, &directory.path().join("b.json")).expect("a download");

        for requests in [first_requests, second_requests] {
            let request = requests.recv().expect("the served request");
            assert!(request.contains("x-api-key: replace-me\r\n"), "request was: {request}");
        }
    }

    #[test]
    fn a_download_without_configured_headers_sends_none_of_them() {
        let (url, requests) = serve_once("200 OK", "{}");
        let directory = TempDir::new().expect("a temporary directory");

        downloader().download_to_file(&url, &directory.path().join("Sensor.json")).expect("a download");

        let request = requests.recv().expect("the served request").to_ascii_lowercase();
        assert!(!request.contains("authorization"), "request was: {request}");
        assert!(!request.contains("x-api-key"), "request was: {request}");
    }

    #[test]
    fn a_download_does_not_ask_for_a_gzip_encoded_body() {
        let (url, requests) = serve_once("200 OK", "{}");
        let directory = TempDir::new().expect("a temporary directory");

        downloader().download_to_file(&url, &directory.path().join("Sensor.json")).expect("a download");

        let request = requests.recv().expect("the served request").to_ascii_lowercase();
        assert!(!request.contains("gzip"), "request was: {request}");
    }

    #[test]
    fn a_download_keeps_the_content_type_its_server_declared() {
        let (url, _requests) = serve_once_with_headers("200 OK", "Content-Type: text/csv; charset=windows-1252\r\n", "id;name\n");
        let directory = TempDir::new().expect("a temporary directory");

        let downloaded = downloader().download_to_file(&url, &directory.path().join("feed.csv")).expect("a download");

        assert_eq!(
            downloaded.content_type().as_ref().map(ToString::to_string).as_deref(),
            Some("text/csv; charset=windows-1252")
        );
    }

    #[test]
    fn a_redirected_download_is_served_from_the_url_redirected_to() {
        let (target, _target_requests) = serve_once("200 OK", "id;name\n");
        let (requested, _requests) = serve_once_with_headers("302 Found", format!("Location: {target}\r\n"), "");
        let directory = TempDir::new().expect("a temporary directory");

        let downloaded = downloader()
            .download_to_file(&requested, &directory.path().join("feed.csv"))
            .expect("a download");

        assert_eq!(downloaded.url(), &target);
        assert_ne!(downloaded.url(), &requested);
    }

    #[test]
    fn an_unredirected_download_is_served_from_the_url_requested() {
        let (url, _requests) = serve_once("200 OK", "id;name\n");
        let directory = TempDir::new().expect("a temporary directory");

        let downloaded = downloader().download_to_file(&url, &directory.path().join("feed.csv")).expect("a download");

        assert_eq!(downloaded.url(), &url);
    }

    #[test]
    fn a_download_without_a_content_type_declares_none() {
        let (url, _requests) = serve_once("200 OK", "id;name\n");
        let directory = TempDir::new().expect("a temporary directory");

        let downloaded = downloader().download_to_file(&url, &directory.path().join("feed.csv")).expect("a download");

        assert!(downloaded.content_type().is_none());
    }

    #[test]
    fn a_source_that_answers_too_many_requests_is_asked_again_until_it_serves_the_file() {
        let mut server = mockito::Server::new();
        let limited = server.mock("GET", "/feed.csv").with_status(429).expect(1).create();
        let delivered = server.mock("GET", "/feed.csv").with_status(200).with_body("id;name\n").expect(1).create();
        let directory = TempDir::new().expect("a temporary directory");

        let downloaded = downloader()
            .download_to_file(&feed(&server), &directory.path().join("feed.csv"))
            .expect("a download");

        limited.assert();
        delivered.assert();
        assert_eq!(read_to_string(downloaded.path()).expect("the written file"), "id;name\n");
    }

    #[test]
    fn a_source_that_keeps_failing_fails_with_its_last_answer_once_the_attempts_are_spent() {
        let mut server = mockito::Server::new();
        let failing = server.mock("GET", "/feed.csv").with_status(503).with_body("upstream busy").expect(3).create();
        let directory = TempDir::new().expect("a temporary directory");
        let destination = directory.path().join("feed.csv");

        let error = downloader().download_to_file(&feed(&server), &destination).expect_err("a failure");

        failing.assert();
        let CollectorError::HttpStatus { status, body, .. } = error else {
            panic!("expected a status failure");
        };
        assert_eq!(status.as_u16(), 503);
        assert_eq!(body.text(), "upstream busy");
        assert!(!destination.exists());
    }

    #[test]
    fn a_client_error_other_than_too_many_requests_is_not_asked_again() {
        let mut server = mockito::Server::new();
        let refused = server.mock("GET", "/feed.csv").with_status(403).with_body("key revoked").expect(1).create();
        let directory = TempDir::new().expect("a temporary directory");

        let error = downloader()
            .download_to_file(&feed(&server), &directory.path().join("feed.csv"))
            .expect_err("a refusal");

        refused.assert();
        assert!(matches!(error, CollectorError::HttpStatus { status, .. } if status.as_u16() == 403));
    }

    #[test]
    fn the_next_attempt_waits_at_least_the_retry_after_the_source_sent() {
        let mut server = mockito::Server::new();
        let limited = server
            .mock("GET", "/feed.csv")
            .with_status(503)
            .with_header("Retry-After", "2")
            .expect(1)
            .create();
        let delivered = server.mock("GET", "/feed.csv").with_status(200).with_body("id;name\n").expect(1).create();
        let warm = server.mock("GET", "/warm.csv").with_status(200).expect(1).create();
        let directory = TempDir::new().expect("a temporary directory");
        let mut downloader = downloader();
        // The first request also builds the HTTP client, which takes long enough to blur the wait.
        let warm_url = MaskedUrl::plain(Url::parse(&format!("{}/warm.csv", server.url())).expect("a valid URL"));
        downloader.download_to_file(&warm_url, &directory.path().join("warm.csv")).expect("a download");
        let started = Instant::now();

        downloader
            .download_to_file(&feed(&server), &directory.path().join("feed.csv"))
            .expect("a download");

        warm.assert();
        limited.assert();
        delivered.assert();
        // The first backoff is 1s, so only the server's own `Retry-After` makes the wait 2s.
        assert!(started.elapsed() >= Duration::from_secs(2), "waited {:?}", started.elapsed());
    }

    #[test]
    fn a_requested_shutdown_ends_the_wait_and_fails_with_the_last_answer() {
        let mut server = mockito::Server::new();
        let failing = server.mock("GET", "/feed.csv").with_status(503).expect(1).create();
        let directory = TempDir::new().expect("a temporary directory");
        let mut downloader = Downloader::new(UserAgent::from("cassiopeia/test".to_owned()), RequestHeaders::default(), &STOPPING);

        let error = downloader
            .download_to_file(&feed(&server), &directory.path().join("feed.csv"))
            .expect_err("a failure");

        failing.assert();
        assert!(matches!(error, CollectorError::HttpStatus { status, .. } if status.as_u16() == 503));
    }
}
