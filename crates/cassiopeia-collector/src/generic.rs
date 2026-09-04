use crate::{
    collector::Collector,
    error::{CollectorError, ERROR_BODY_CAP},
    file_extension::FileExtension,
    source::CollectorSource,
};
use cassiopeia_common::{captured_body::CapturedBody, channel::ChannelSender, format::DataFormat, input::Input, signal::Signal};
use cassiopeia_ir::payload::{CollectedPayload, FilePayload};
use reqwest::blocking::Client;
use std::{
    env,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};
use temp_dir::TempDir;
use tracing::debug;
use url::Url;

/// A generic collector that handles downloading from URLs and resolving local paths.
pub struct GenericCollector {
    source: CollectorSource,
    format_hint: Option<DataFormat>,
}

impl GenericCollector {
    /// Creates a collector for the given source, tagging every payload with `format_hint`.
    #[must_use]
    pub const fn new(source: CollectorSource, format_hint: Option<DataFormat>) -> GenericCollector {
        GenericCollector { source, format_hint }
    }

    /// Resolves a single input to a local file and sends it as a `CollectedPayload`.
    ///
    /// A `Local` input is used in place; a `Remote` input is downloaded into `temp_dir`.
    fn collect_single(
        input: &Input,
        extension: &FileExtension,
        temp_dir: &Path,
        format_hint: Option<DataFormat>,
        sender: &ChannelSender<Signal<CollectedPayload, CollectorError>>,
    ) -> Result<(), CollectorError> {
        let path = match input {
            Input::Local(path) => path.clone(),
            Input::Remote(url) => {
                let client = Client::new();
                let dest = temp_dir.join(format!("input.{extension}"));
                let path = download_to_file(&client, url, &dest)?;
                debug!("Downloaded {} to {}", url, path.display());
                path
            }
        };

        let payload = CollectedPayload::File(FilePayload::new(path, format_hint));

        sender.send(Signal::Data(payload)).map_err(|_| CollectorError::ChannelClosed)?;

        Ok(())
    }
}

impl Collector for GenericCollector {
    fn collect(self: Box<Self>, sender: ChannelSender<Signal<CollectedPayload, CollectorError>>) -> Result<(), CollectorError> {
        let temp_dir = TempDir::new().map_err(|source| CollectorError::Io { source, path: env::temp_dir() })?;
        // Remote downloads must outlive the collector because downstream stages read them later.
        // The collector does not remove them; cleanup is the caller's responsibility.
        let temp_dir = temp_dir.dont_delete_on_drop();

        match &self.source {
            CollectorSource::Single { input, extension } => {
                Self::collect_single(input, extension, temp_dir.path(), self.format_hint, &sender)?;
            }
            CollectorSource::Multiple { inputs, extension } => {
                for (i, input) in inputs.iter().enumerate() {
                    let sub_dir = temp_dir.path().join(format!("input_{i}"));
                    fs::create_dir_all(&sub_dir).map_err(|source| CollectorError::Io { source, path: sub_dir.clone() })?;
                    Self::collect_single(input, extension, &sub_dir, self.format_hint, &sender)?;
                }
            }
        }

        Ok(())
    }
}

/// Downloads a single file from a URL to a destination path, returning that path.
///
/// # Errors
///
/// Returns [`CollectorError::Http`] when the request never completes,
/// [`CollectorError::HttpStatus`] when the server answers with a non-success status, and
/// [`CollectorError::Io`] when the response body cannot be written to `dest`.
pub fn download_to_file(client: &Client, url: &Url, dest: &Path) -> Result<PathBuf, CollectorError> {
    let response = client.get(url.as_str()).send().map_err(|source| CollectorError::Http {
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

#[cfg(test)]
mod tests {
    use crate::{
        collector::Collector,
        error::CollectorError,
        file_extension::FileExtension,
        generic::{GenericCollector, download_to_file},
        source::CollectorSource,
    };
    use cassiopeia_common::{
        channel::{ChannelPolicy, channel},
        format::DataFormat,
        input::Input,
        signal::Signal,
    };
    use cassiopeia_ir::payload::CollectedPayload;
    use reqwest::blocking::Client;
    use std::{
        fs::read_to_string,
        io::{Read as _, Write as _},
        net::TcpListener,
        path::PathBuf,
        thread,
    };
    use tempfile::TempDir;
    use url::Url;

    /// Serves exactly one HTTP response with the given status line and body, then closes.
    fn serve_once(status_line: &'static str, body: &'static str) -> Url {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a bindable port");
        let port = listener.local_addr().expect("a local address").port();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0_u8; 1024];
                let _ = stream.read(&mut buffer);
                let response = format!("HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        Url::parse(&format!("http://127.0.0.1:{port}/Sensor.json")).expect("a valid URL")
    }

    fn extension(value: &str) -> FileExtension {
        FileExtension::new(value).expect("valid extension")
    }

    #[test]
    fn a_single_local_input_is_sent_as_a_file_payload() {
        let source = CollectorSource::Single {
            input: Input::Local(PathBuf::from("data/stations.csv")),
            extension: extension("csv"),
        };
        let collector = GenericCollector::new(source, Some(DataFormat::Csv));
        let (tx, rx) = channel(ChannelPolicy::Unbounded);

        Box::new(collector).collect(tx).unwrap();

        let signal = rx.recv().unwrap();
        let Signal::Data(CollectedPayload::File(payload)) = signal else {
            panic!("expected a file payload");
        };
        assert_eq!(payload.path(), &PathBuf::from("data/stations.csv"));
        assert_eq!(payload.format_hint(), &Some(DataFormat::Csv));
        assert!(rx.recv().is_err());
    }

    #[test]
    fn multiple_local_inputs_each_produce_a_payload() {
        let source = CollectorSource::Multiple {
            inputs: vec![Input::Local(PathBuf::from("a.json")), Input::Local(PathBuf::from("b.json"))],
            extension: extension("json"),
        };
        let collector = GenericCollector::new(source, None);
        let (tx, rx) = channel(ChannelPolicy::Unbounded);

        Box::new(collector).collect(tx).unwrap();

        let received: Vec<PathBuf> = rx
            .iter()
            .map(|signal| {
                let Signal::Data(CollectedPayload::File(payload)) = signal else {
                    panic!("expected a file payload");
                };
                payload.into_path()
            })
            .collect();
        assert_eq!(received, vec![PathBuf::from("a.json"), PathBuf::from("b.json")]);
    }

    #[test]
    fn the_format_hint_is_propagated_to_every_payload() {
        let source = CollectorSource::Single {
            input: Input::Local(PathBuf::from("input.geojson")),
            extension: extension("geojson"),
        };
        let collector = GenericCollector::new(source, Some(DataFormat::GeoJson));
        let (tx, rx) = channel(ChannelPolicy::Unbounded);

        Box::new(collector).collect(tx).unwrap();

        let Signal::Data(CollectedPayload::File(payload)) = rx.recv().unwrap() else {
            panic!("expected a file payload");
        };
        assert_eq!(payload.format_hint(), &Some(DataFormat::GeoJson));
    }

    #[test]
    fn a_refused_download_keeps_the_servers_own_explanation() {
        let url = serve_once("404 Not Found", "no schema published under that name");
        let directory = TempDir::new().expect("a temporary directory");

        let error = download_to_file(&Client::new(), &url, &directory.path().join("Sensor.json")).expect_err("a refusal");

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
        let url = serve_once("500 Internal Server Error", "upstream unavailable");
        let directory = TempDir::new().expect("a temporary directory");
        let destination = directory.path().join("Sensor.json");

        let _ = download_to_file(&Client::new(), &url, &destination).expect_err("a refusal");

        assert!(!destination.exists());
    }

    #[test]
    fn a_successful_download_writes_the_body() {
        let url = serve_once("200 OK", r#"{"type":"object"}"#);
        let directory = TempDir::new().expect("a temporary directory");
        let destination = directory.path().join("Sensor.json");

        let path = download_to_file(&Client::new(), &url, &destination).expect("a download");

        assert_eq!(read_to_string(path).expect("the written file"), r#"{"type":"object"}"#);
    }
}
