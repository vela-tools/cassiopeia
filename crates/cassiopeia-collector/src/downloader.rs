use crate::error::{CollectorError, ERROR_BODY_CAP};
use cassiopeia_common::{captured_body::CapturedBody, user_agent::UserAgent};
use reqwest::blocking::Client;
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};
use url::Url;

/// Fetches remote sources over HTTP, announcing every request with one fixed `User-Agent`.
///
/// The HTTP client is built on the first download rather than at construction: a blocking reqwest
/// client eagerly spawns its own runtime thread, and a run whose sources are all local files never
/// reaches the network.
pub struct Downloader {
    /// The identifier every request announces itself with.
    user_agent: UserAgent,
    /// The blocking HTTP client, built on the first download and reused by every later one.
    client: Option<Client>,
}

impl Downloader {
    /// Creates a downloader that identifies itself as `user_agent` and has not yet touched the
    /// network.
    #[must_use]
    pub const fn new(user_agent: UserAgent) -> Downloader {
        Downloader { user_agent, client: None }
    }

    /// Downloads a single file from `url` to `dest`, returning that path.
    ///
    /// # Errors
    ///
    /// Returns [`CollectorError::ClientInit`] when the HTTP client cannot be built,
    /// [`CollectorError::Http`] when the request never completes, [`CollectorError::HttpStatus`]
    /// when the server answers with a non-success status, and [`CollectorError::Io`] when the
    /// response body cannot be written to `dest`.
    pub fn download_to_file(&mut self, url: &Url, dest: &Path) -> Result<PathBuf, CollectorError> {
        let response = self.client()?.get(url.as_str()).send().map_err(|source| CollectorError::Http {
            source,
            // The error owns the URL after the borrowed request is dropped.
            url: url.clone(),
        })?;

        // A 4xx/5xx answers with an error page, not the resource; treating that body as the download
        // would silently write garbage in place of the file, so a non-success status is a failure. The
        // status is checked here rather than through `error_for_status`, which consumes the response and
        // throws the body away, and that body is the server's only explanation of the refusal.
        let status = response.status();
        let body = match response.bytes() {
            Ok(bytes) => CapturedBody::capped(bytes.to_vec(), ERROR_BODY_CAP.max(bytes.len())),
            Err(source) => {
                if status.is_success() {
                    return Err(CollectorError::Http { source, url: url.clone() });
                }
                CapturedBody::unreadable(&source)
            }
        };
        if !status.is_success() {
            return Err(CollectorError::HttpStatus {
                url: url.clone(),
                status,
                body: Box::new(body.echo(ERROR_BODY_CAP)),
            });
        }
        let body = body.bytes();
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
        Ok(dest.to_path_buf())
    }

    /// The HTTP client, built with this downloader's `User-Agent` on first use and reused after.
    fn client(&mut self) -> Result<&Client, CollectorError> {
        let client = match self.client.take() {
            Some(client) => client,
            None => Client::builder()
                .user_agent(self.user_agent.as_str())
                .build()
                .map_err(|source| CollectorError::ClientInit { source })?,
        };

        Ok(self.client.insert(client))
    }
}

#[cfg(test)]
mod tests {
    use crate::{downloader::Downloader, error::CollectorError};
    use cassiopeia_common::user_agent::UserAgent;
    use std::{
        fs::read_to_string,
        io::{Read as _, Write as _},
        net::TcpListener,
        sync::mpsc::{Receiver, channel},
        thread,
    };
    use tempfile::TempDir;
    use url::Url;

    /// Serves exactly one HTTP response with the given status line and body, then closes. The
    /// receiver hands back the raw request the server read, so a test can assert on its headers.
    fn serve_once(status_line: &'static str, body: &'static str) -> (Url, Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a bindable port");
        let port = listener.local_addr().expect("a local address").port();
        let (request_tx, request_rx) = channel();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0_u8; 1024];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let _ = request_tx.send(String::from_utf8_lossy(&buffer[..read]).into_owned());
                let response = format!("HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (Url::parse(&format!("http://127.0.0.1:{port}/Sensor.json")).expect("a valid URL"), request_rx)
    }

    fn downloader() -> Downloader {
        Downloader::new(UserAgent::from("cassiopeia/test".to_owned()))
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
        assert_eq!(named, url);
        assert_eq!(status.as_u16(), 404);
        assert_eq!(body.text(), "no schema published under that name");
    }

    #[test]
    fn a_refused_download_writes_no_file() {
        // A 4xx answers with an error page, not the resource; writing it would leave garbage behind.
        let (url, _requests) = serve_once("500 Internal Server Error", "upstream unavailable");
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

        let path = downloader().download_to_file(&url, &destination).expect("a download");

        assert_eq!(read_to_string(path).expect("the written file"), r#"{"type":"object"}"#);
    }

    #[test]
    fn a_download_announces_the_user_agent_it_was_built_with() {
        let (url, requests) = serve_once("200 OK", "{}");
        let directory = TempDir::new().expect("a temporary directory");

        downloader().download_to_file(&url, &directory.path().join("Sensor.json")).expect("a download");

        let request = requests.recv().expect("the served request");
        assert!(request.contains("user-agent: cassiopeia/test"), "request was: {request}");
    }
}
