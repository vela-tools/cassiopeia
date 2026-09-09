use crate::{collector::Collector, downloader::Downloader, error::CollectorError, file_extension::FileExtension, source::CollectorSource};
use cassiopeia_common::{channel::ChannelSender, format::DataFormat, input::Input, signal::Signal, user_agent::UserAgent};
use cassiopeia_ir::payload::{CollectedPayload, FilePayload};
use std::{
    env,
    fs,
    path::{Path, PathBuf},
};
use temp_dir::TempDir;
use tracing::debug;

/// A generic collector that handles downloading from URLs and resolving local paths.
pub struct GenericCollector {
    source: CollectorSource,
    format_hint: Option<DataFormat>,
    user_agent: UserAgent,
}

impl GenericCollector {
    /// Creates a collector for the given source, tagging every payload with `format_hint` and
    /// identifying itself as `user_agent` on every remote fetch.
    #[must_use]
    pub const fn new(source: CollectorSource, format_hint: Option<DataFormat>, user_agent: UserAgent) -> GenericCollector {
        GenericCollector {
            source,
            format_hint,
            user_agent,
        }
    }
}

impl Collector for GenericCollector {
    fn collect(self: Box<Self>, sender: ChannelSender<Signal<CollectedPayload, CollectorError>>) -> Result<(), CollectorError> {
        let GenericCollector {
            source,
            format_hint,
            user_agent,
        } = *self;
        let temp_dir = TempDir::new().map_err(|source| CollectorError::Io { source, path: env::temp_dir() })?;
        // Remote downloads must outlive the collector because downstream stages read them later.
        // The collector does not remove them; cleanup is the caller's responsibility.
        let temp_dir = temp_dir.dont_delete_on_drop();
        // One downloader serves every input of the lane, so a multi-source lane opens a single
        // connection pool rather than one per source.
        let mut downloader = Downloader::new(user_agent);

        match &source {
            CollectorSource::Single { input, extension } => {
                let path = resolve_input(input, extension, temp_dir.path(), &mut downloader)?;
                send_payload(path, format_hint, &sender)?;
            }
            CollectorSource::Multiple { inputs, extension } => {
                for (index, input) in inputs.iter().enumerate() {
                    let sub_dir = temp_dir.path().join(format!("input_{index}"));
                    fs::create_dir_all(&sub_dir).map_err(|source| CollectorError::Io { source, path: sub_dir.clone() })?;
                    let path = resolve_input(input, extension, &sub_dir, &mut downloader)?;
                    send_payload(path, format_hint, &sender)?;
                }
            }
        }

        Ok(())
    }
}

/// Resolves a single input to a local file.
///
/// A `Local` input is used in place; a `Remote` input is downloaded into `temp_dir` under a name
/// `extension` gives it.
fn resolve_input(input: &Input, extension: &FileExtension, temp_dir: &Path, downloader: &mut Downloader) -> Result<PathBuf, CollectorError> {
    match input {
        Input::Local(path) => Ok(path.clone()),
        Input::Remote(url) => {
            let destination = temp_dir.join(format!("input.{extension}"));
            let path = downloader.download_to_file(url, &destination)?;
            debug!("Downloaded {} to {}", url, path.display());
            Ok(path)
        }
    }
}

/// Sends one resolved file downstream as a `CollectedPayload`.
fn send_payload(
    path: PathBuf,
    format_hint: Option<DataFormat>,
    sender: &ChannelSender<Signal<CollectedPayload, CollectorError>>,
) -> Result<(), CollectorError> {
    let payload = CollectedPayload::File(FilePayload::new(path, format_hint));

    sender.send(Signal::Data(payload)).map_err(|_| CollectorError::ChannelClosed)
}

#[cfg(test)]
mod tests {
    use crate::{collector::Collector, file_extension::FileExtension, generic::GenericCollector, source::CollectorSource};
    use cassiopeia_common::{
        channel::{ChannelPolicy, channel},
        format::DataFormat,
        input::Input,
        signal::Signal,
        user_agent::UserAgent,
    };
    use cassiopeia_ir::payload::CollectedPayload;
    use std::path::PathBuf;

    fn extension(value: &str) -> FileExtension {
        FileExtension::new(value).expect("valid extension")
    }

    fn collector(source: CollectorSource, format_hint: Option<DataFormat>) -> Box<GenericCollector> {
        Box::new(GenericCollector::new(source, format_hint, UserAgent::from("cassiopeia/test".to_owned())))
    }

    #[test]
    fn a_single_local_input_is_sent_as_a_file_payload() {
        let source = CollectorSource::Single {
            input: Input::Local(PathBuf::from("data/stations.csv")),
            extension: extension("csv"),
        };
        let (tx, rx) = channel(ChannelPolicy::Unbounded);

        collector(source, Some(DataFormat::Csv)).collect(tx).unwrap();

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
        let (tx, rx) = channel(ChannelPolicy::Unbounded);

        collector(source, None).collect(tx).unwrap();

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
        let (tx, rx) = channel(ChannelPolicy::Unbounded);

        collector(source, Some(DataFormat::GeoJson)).collect(tx).unwrap();

        let Signal::Data(CollectedPayload::File(payload)) = rx.recv().unwrap() else {
            panic!("expected a file payload");
        };
        assert_eq!(payload.format_hint(), &Some(DataFormat::GeoJson));
    }
}
