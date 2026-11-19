use crate::{
    collection_end::CollectionEnd,
    collection_warnings::CollectionWarnings,
    collector::Collector,
    downloader::Downloader,
    error::CollectorError,
    scratch_files::ScratchFiles,
    source::CollectorSource,
};
use cassiopeia_common::{channel::ChannelSender, format::DataFormat, input::Input, signal::Signal};
use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
use cassiopeia_ir::payload::{CollectedPayload, FilePayload};
use std::path::PathBuf;
use tracing::debug;

/// A generic collector that handles downloading from URLs and resolving local paths.
pub struct GenericCollector {
    source: CollectorSource,
    format_hint: Option<DataFormat>,
    /// The downloader every remote input of the lane is fetched through, so a multi-source lane
    /// opens a single connection pool rather than one per source.
    downloader: Downloader,
}

impl GenericCollector {
    /// Creates a collector for the given source, tagging every payload with `format_hint` and
    /// fetching every remote input through `downloader`.
    #[must_use]
    pub const fn new(source: CollectorSource, format_hint: Option<DataFormat>, downloader: Downloader) -> GenericCollector {
        GenericCollector {
            source,
            format_hint,
            downloader,
        }
    }
}

impl Collector for GenericCollector {
    fn collect(
        self: Box<Self>,
        scratch: ScratchFiles,
        sender: ChannelSender<Signal<CollectedPayload, CollectorError>>,
    ) -> Result<CollectionEnd, CollectorError> {
        let GenericCollector {
            source,
            format_hint,
            mut downloader,
        } = *self;

        match collect_source(&source, format_hint, &scratch, &mut downloader, &sender) {
            // Files are read as they are found, so collecting them meets nothing to warn about.
            Ok(()) => Ok(CollectionEnd::Collected(CollectionWarnings::default())),
            Err(error) => CollectionEnd::unwound_by(error),
        }
    }
}

/// Resolves every input of `source` and hands each on through `sender` in the order the source lists
/// them, downloading a remote one into the scratch file `scratch` names for it.
fn collect_source(
    source: &CollectorSource,
    format_hint: Option<DataFormat>,
    scratch: &ScratchFiles,
    downloader: &mut Downloader,
    sender: &ChannelSender<Signal<CollectedPayload, CollectorError>>,
) -> Result<(), CollectorError> {
    match source {
        CollectorSource::Single { input, extension } => {
            let payload = resolve_input(input, format_hint, || scratch.only(extension), downloader)?;
            send_payload(payload, sender)
        }
        CollectorSource::Multiple { inputs, extension } => inputs.iter().enumerate().try_for_each(|(index, input)| {
            let payload = resolve_input(input, format_hint, || scratch.numbered(index, extension), downloader)?;
            send_payload(payload, sender)
        }),
    }
}

/// Resolves a single input to the payload of a local file, with what its source says about its
/// encoding.
///
/// A `Local` input is used in place, carries no hint, and writes nothing. A `Remote` input is
/// downloaded to the scratch file `destination` names, keeps the URL it was requested by as its
/// source, and carries what the server it was finally served from says: its declared `charset` and
/// its domain, which the local copy no longer holds. A redirect can hand the request to another
/// server, and the body is that server's, so its domain is the one that hints at the body's encoding.
fn resolve_input(
    input: &Input,
    format_hint: Option<DataFormat>,
    destination: impl FnOnce() -> PathBuf,
    downloader: &mut Downloader,
) -> Result<FilePayload, CollectorError> {
    match input {
        // The payload owns its path, while the input stays with the collector's source.
        Input::Local(path) => Ok(FilePayload::local(path.clone(), format_hint, EncodingHint::NONE)),
        Input::Remote(url) => {
            let file = downloader.download_to_file(url, &destination())?;
            debug!("Downloaded {} from {} to {}", url, file.url(), file.path().display());
            let encoding_hint = EncodingHint::for_remote_source(file.url().url(), file.content_type().as_ref());
            // The payload owns the URL it is named by, while the input stays with the collector's source.
            Ok(FilePayload::downloaded(file.into_path(), url.clone(), format_hint, encoding_hint))
        }
    }
}

/// Sends one resolved file downstream as a `CollectedPayload`.
fn send_payload(payload: FilePayload, sender: &ChannelSender<Signal<CollectedPayload, CollectorError>>) -> Result<(), CollectorError> {
    sender
        .send(Signal::Data(CollectedPayload::File(payload)))
        .map_err(|_| CollectorError::ChannelClosed)
}

#[cfg(test)]
mod tests {
    use crate::{
        collector::Collector,
        downloader::Downloader,
        error::CollectorError,
        file_extension::FileExtension,
        generic::GenericCollector,
        scratch_files::ScratchFiles,
        source::CollectorSource,
    };
    use cassiopeia_common::{
        channel::{ChannelPolicy, ChannelReceiver, channel},
        format::DataFormat,
        input::Input,
        masked_url::MaskedUrl,
        request_header::RequestHeaders,
        signal::Signal,
        user_agent::UserAgent,
    };
    use cassiopeia_ir::payload::{CollectedPayload, FilePayload};
    use mockito::ServerGuard;
    use pretty_assertions::assert_eq;
    use std::{fs, path::PathBuf, sync::atomic::AtomicBool};
    use tempfile::TempDir;
    use url::Url;

    /// A shutdown flag never set, as in a run nobody stops.
    static RUNNING: AtomicBool = AtomicBool::new(false);

    fn extension(value: &str) -> FileExtension {
        FileExtension::new(value).expect("valid extension")
    }

    fn collector(source: CollectorSource, format_hint: Option<DataFormat>) -> Box<GenericCollector> {
        Box::new(GenericCollector::new(
            source,
            format_hint,
            Downloader::new(UserAgent::from("cassiopeia/test".to_owned()), RequestHeaders::default(), &RUNNING),
        ))
    }

    /// The files of the collector numbered `collector` in the scratch folder `scratch`.
    fn files_in(scratch: &TempDir, collector: u32) -> ScratchFiles {
        ScratchFiles::new(scratch.path().into(), collector)
    }

    /// The name of every entry `scratch` holds, in order.
    fn entries(scratch: &TempDir) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(scratch.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// Every file payload `receiver` was sent, in the order it was sent.
    fn file_payloads(receiver: ChannelReceiver<Signal<CollectedPayload, CollectorError>>) -> Vec<FilePayload> {
        receiver
            .into_iter()
            .map(|signal| {
                let Signal::Data(CollectedPayload::File(payload)) = signal else {
                    panic!("expected a file payload");
                };
                payload
            })
            .collect()
    }

    /// The input of the document the mock server serves at `path`.
    fn remote(server: &ServerGuard, path: &str) -> Input {
        Input::Remote(MaskedUrl::plain(Url::parse(&format!("{}{path}", server.url())).unwrap()))
    }

    #[test]
    fn a_single_local_input_is_sent_as_a_file_payload_and_writes_nothing_to_the_scratch_folder() {
        let source = CollectorSource::Single {
            input: Input::Local(PathBuf::from("data/stations.csv")),
            extension: extension("csv"),
        };
        let scratch = TempDir::new().unwrap();
        let (tx, rx) = channel(ChannelPolicy::Unbounded);

        collector(source, Some(DataFormat::Csv)).collect(files_in(&scratch, 0), tx).unwrap();

        let [payload] = <[FilePayload; 1]>::try_from(file_payloads(rx)).unwrap();
        assert_eq!(payload.path(), &PathBuf::from("data/stations.csv"));
        assert_eq!(payload.format_hint(), &Some(DataFormat::Csv));
        assert_eq!(entries(&scratch), Vec::<String>::new());
    }

    #[test]
    fn multiple_local_inputs_each_produce_a_payload_and_write_nothing_to_the_scratch_folder() {
        let source = CollectorSource::Multiple {
            inputs: vec![Input::Local(PathBuf::from("a.json")), Input::Local(PathBuf::from("b.json"))],
            extension: extension("json"),
        };
        let scratch = TempDir::new().unwrap();
        let (tx, rx) = channel(ChannelPolicy::Unbounded);

        collector(source, None).collect(files_in(&scratch, 0), tx).unwrap();

        let received: Vec<PathBuf> = file_payloads(rx).into_iter().map(FilePayload::into_path).collect();
        assert_eq!(received, vec![PathBuf::from("a.json"), PathBuf::from("b.json")]);
        assert_eq!(entries(&scratch), Vec::<String>::new());
    }

    #[test]
    fn the_format_hint_is_propagated_to_every_payload() {
        let source = CollectorSource::Single {
            input: Input::Local(PathBuf::from("input.geojson")),
            extension: extension("geojson"),
        };
        let scratch = TempDir::new().unwrap();
        let (tx, rx) = channel(ChannelPolicy::Unbounded);

        collector(source, Some(DataFormat::GeoJson)).collect(files_in(&scratch, 0), tx).unwrap();

        let [payload] = <[FilePayload; 1]>::try_from(file_payloads(rx)).unwrap();
        assert_eq!(payload.format_hint(), &Some(DataFormat::GeoJson));
    }

    #[test]
    fn a_single_remote_input_is_downloaded_to_its_collectors_file_in_the_scratch_folder() {
        let mut server = mockito::Server::new();
        let _download = server.mock("GET", "/stations.csv").with_body("id\nS1\n").expect(1).create();
        let source = CollectorSource::Single {
            input: remote(&server, "/stations.csv"),
            extension: extension("csv"),
        };
        let scratch = TempDir::new().unwrap();
        let (tx, rx) = channel(ChannelPolicy::Unbounded);

        collector(source, None).collect(files_in(&scratch, 3), tx).unwrap();

        let [payload] = <[FilePayload; 1]>::try_from(file_payloads(rx)).unwrap();
        assert_eq!(payload.path(), &scratch.path().join("input-3.csv"));
        assert_eq!(payload.source().to_string(), format!("{}/stations.csv", server.url()));
        assert_eq!(fs::read_to_string(payload.path()).unwrap(), "id\nS1\n");
        assert_eq!(entries(&scratch), ["input-3.csv"]);
    }

    #[test]
    fn multiple_remote_inputs_are_each_downloaded_to_a_numbered_file_in_the_scratch_folder() {
        let mut server = mockito::Server::new();
        let _first = server.mock("GET", "/a.json").with_body("[1]").expect(1).create();
        let _second = server.mock("GET", "/b.json").with_body("[2]").expect(1).create();
        let source = CollectorSource::Multiple {
            inputs: vec![remote(&server, "/a.json"), remote(&server, "/b.json")],
            extension: extension("json"),
        };
        let scratch = TempDir::new().unwrap();
        let (tx, rx) = channel(ChannelPolicy::Unbounded);

        collector(source, None).collect(files_in(&scratch, 3), tx).unwrap();

        let contents: Vec<String> = file_payloads(rx).iter().map(|payload| fs::read_to_string(payload.path()).unwrap()).collect();
        assert_eq!(contents, ["[1]", "[2]"]);
        assert_eq!(entries(&scratch), ["input-3-0.json", "input-3-1.json"]);
    }
}
