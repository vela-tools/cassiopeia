use crate::{
    error::{PipelineError, Result},
    input_config::InputConfig,
};
use cassiopeia_collector::{downloader::Downloader, error::CollectorError};
use cassiopeia_common::{schema_source::SchemaSource, user_agent::UserAgent};
use cassiopeia_expander::router::MappingRouter;
use cassiopeia_manifest::mapping_binding::MappingBinding;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use cassiopeia_reporter::reporter::MessageLog;
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
};
use temp_dir::TempDir;
use url::Url;

/// The custom validation schemas resolved for one run: each entity type mapped to the on-disk schema
/// file that validates it, plus the temporary directory any downloaded remote schema was written
/// into.
pub(crate) struct ResolvedSchemas {
    /// The explicit schema file for each entity type that named one, per-input or global.
    pub(crate) by_type: HashMap<NameBuf, PathBuf>,
    /// Holds any downloaded remote schemas on disk. It is kept alive until validation finishes so
    /// lazy schema compilation can still read the files; dropping it deletes them. Never read
    /// directly: its only job is to own the directory for the run's lifetime.
    _cache: Option<TempDir>,
}

/// Resolves the custom validation schema each entity type is checked against, downloading any remote
/// source once at run setup so the validator only ever compiles from a filesystem path.
///
/// Precedence is per-input over global, mirroring `@context`: an input's own `schema` binds every
/// type its lane produces, and the `global` source then fills in every produced type that named none.
/// A local source is absolutized against the file that declared it (the input's mapping directory,
/// or the process working directory for the global one) without checking that it exists, so a typo
/// aborts later through the validator's normal missing-schema error rather than here. A remote source
/// is downloaded into a run-scoped temporary directory, under the run's default `User-Agent`, and
/// thereafter treated as that local file.
///
/// # Errors
///
/// Returns [`PipelineError::SchemaFetch`] when a remote source cannot be downloaded; a fetch failure
/// at setup is fatal.
pub(crate) fn resolve_custom_schemas(
    loaded: &[(&InputConfig, MappingRouter)],
    global: Option<&SchemaSource>,
    reporter: &dyn MessageLog,
    user_agent: &UserAgent,
) -> Result<ResolvedSchemas> {
    let mut downloader = SchemaDownloader::new(reporter, user_agent.clone());
    let mut by_type = HashMap::new();

    // A per-input schema binds every type its lane produces and takes precedence over the global one.
    for (input, router) in loaded {
        let Some(source) = input.schema.as_ref() else {
            continue;
        };
        let base = input_schema_base(&input.mapping_binding);
        for mapping in router.mappings() {
            let entity_type = mapping.data_model().entity_type();
            let path = downloader.resolve(source, base, entity_type)?;
            // The map key must own the type name; cloning it once per bound type is unavoidable.
            by_type.insert(entity_type.clone(), path);
        }
    }

    // The global schema fills in every produced type no per-input schema already covers.
    if let Some(source) = global {
        let base = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        for (_, router) in loaded {
            for mapping in router.mappings() {
                let entity_type = mapping.data_model().entity_type();
                if by_type.contains_key(entity_type) {
                    continue;
                }
                let path = downloader.resolve(source, &base, entity_type)?;
                by_type.insert(entity_type.clone(), path);
            }
        }
    }

    Ok(ResolvedSchemas {
        by_type,
        _cache: downloader.into_cache(),
    })
}

/// The directory a per-input local schema is resolved against: the parent of the lane's mapping, so a
/// bare filename reads next to the mapping, the same base the input's `mapping:` resolves against.
fn input_schema_base(binding: &MappingBinding) -> &Path {
    let mapping = match binding {
        MappingBinding::Single { mapping } => Some(mapping.as_path()),
        MappingBinding::Collections { mappings } => mappings.first().map(|entry| entry.mapping().as_path()),
    };
    mapping.and_then(Path::parent).unwrap_or_else(|| Path::new(""))
}

/// Fetches remote schema sources into one run-scoped temporary directory, created lazily on the
/// first download and reused for every subsequent one. A given URL is downloaded only once.
struct SchemaDownloader<'a> {
    /// The reporter a download is announced to.
    reporter: &'a dyn MessageLog,
    /// The temporary directory holding downloaded schemas, created on the first remote source.
    cache: Option<TempDir>,
    /// The HTTP downloader every remote schema is fetched through.
    downloader: Downloader,
    /// Every already-downloaded URL and the file it landed in, so a repeated URL is fetched once.
    fetched: HashMap<Url, PathBuf>,
}

impl<'a> SchemaDownloader<'a> {
    /// Builds a downloader that has not yet touched the network, fetching under `user_agent`.
    fn new(reporter: &'a dyn MessageLog, user_agent: UserAgent) -> SchemaDownloader<'a> {
        SchemaDownloader {
            reporter,
            cache: None,
            downloader: Downloader::new(user_agent),
            fetched: HashMap::new(),
        }
    }

    /// Resolves one source to an on-disk path: a local source is absolutized against `base`; a remote
    /// source is downloaded once into the run's temporary directory.
    fn resolve(&mut self, source: &SchemaSource, base: &Path, entity_type: &NameBuf) -> Result<PathBuf> {
        match source {
            SchemaSource::Local(path) => Ok(absolutize(base, path)),
            SchemaSource::Remote(url) => self.download(url, entity_type),
        }
    }

    /// Downloads a remote schema into the run's temporary directory, reusing the file a prior fetch of
    /// the same URL produced.
    fn download(&mut self, url: &Url, entity_type: &NameBuf) -> Result<PathBuf> {
        if let Some(existing) = self.fetched.get(url) {
            return Ok(existing.clone());
        }

        self.reporter
            .debug(&format!("Fetching custom validation schema for '{entity_type}' from {url}"));

        // Take the cache out so the download borrows a local directory, then restore it after.
        let directory = match self.cache.take() {
            Some(directory) => directory,
            None => TempDir::new().map_err(|source| PipelineError::SchemaFetch {
                url: url.clone(),
                source: Box::new(CollectorError::Io { source, path: env::temp_dir() }),
            })?,
        };
        let destination = directory.path().join(format!("{entity_type}.json"));
        let outcome = self
            .downloader
            .download_to_file(url, &destination)
            .map_err(|source| PipelineError::SchemaFetch {
                url: url.clone(),
                source: Box::new(source),
            });

        self.cache = Some(directory);

        let path = outcome?;
        // The cache key must own the URL; cloning it once per distinct fetched source is unavoidable.
        self.fetched.insert(url.clone(), path.clone());
        Ok(path)
    }

    /// Yields the temporary directory the resolved schemas must outlive, if any download happened.
    fn into_cache(self) -> Option<TempDir> {
        self.cache
    }
}

/// Absolutizes a local schema path against the directory that declared it; an already-absolute path
/// is taken as-is.
fn absolutize(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { base.join(path) }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::PipelineError,
        input_config::{InputConfig, build_input_config},
        schema_resolution::resolve_custom_schemas,
    };
    use cassiopeia_common::{input::Input, schema_source::SchemaSource, user_agent::UserAgent};
    use cassiopeia_expander::router::MappingRouter;
    use cassiopeia_manifest::{input::ManifestInput, mapping_binding::MappingBinding};
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use cassiopeia_reporter::backend::noop::NoopReporter;
    use std::{
        fs,
        io::{Read, Write},
        net::TcpListener,
        path::{Path, PathBuf},
        str::FromStr,
        sync::Arc,
        thread,
    };

    /// Builds one input lane whose mapping sits at `mapping_path` and that names `schema` (if any),
    /// paired with a single-mapping router producing `data_model`. The mapping path is what a bare
    /// per-input schema is resolved next to; the router carries the produced entity type.
    fn lane(mapping_path: &str, data_model: &str, schema: Option<&str>) -> (InputConfig, MappingRouter) {
        let input = ManifestInput::builder()
            .source(Input::from_str("data.csv").unwrap())
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from(mapping_path),
            })
            .schema(schema.map(|value| SchemaSource::from_str(value).unwrap()))
            .build();
        let config = build_input_config(&input, &serde_json::Map::new()).unwrap();

        let document = format!(
            r#"{{ version: "v4", dataModel: "{data_model}", identity: {{ entityName: "E-{{{{ id }}}}" }}, attributes: {{ v: {{ source: "{{{{ v }}}}" }} }} }}"#
        );
        let mut runner = TemplateRunner::new();
        let mapping = Mapping::from_json5(&document, Path::new(mapping_path), &mut runner).unwrap();
        (config, MappingRouter::Single(Arc::new(mapping)))
    }

    /// The run's build-time identifier, which every schema fetch announces itself with.
    fn user_agent() -> UserAgent {
        UserAgent::from("cassiopeia/1.0.0".to_owned())
    }

    fn exoplanet() -> NameBuf {
        NameBuf::new("ExoPlanet").unwrap()
    }

    /// Serves exactly one HTTP response with the given status line and body, then closes.
    fn serve_once(status_line: &'static str, body: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0_u8; 1024];
                let _ = stream.read(&mut buffer);
                let response = format!("HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        port
    }

    #[test]
    fn a_per_input_schema_resolves_relative_to_its_mapping_file() {
        let (config, router) = lane("/maps/planet.json5", "ExoPlanet", Some("e.json"));
        let loaded = vec![(&config, router)];
        let reporter = NoopReporter::new();

        let resolved = resolve_custom_schemas(&loaded, None, &reporter, &user_agent()).unwrap();

        assert_eq!(resolved.by_type.get(&exoplanet()), Some(&PathBuf::from("/maps/e.json")));
    }

    #[test]
    fn the_global_schema_fills_types_without_a_per_input_schema() {
        let (config, router) = lane("/maps/planet.json5", "ExoPlanet", None);
        let loaded = vec![(&config, router)];
        let reporter = NoopReporter::new();
        let global = SchemaSource::Local(PathBuf::from("/schemas/g.json"));

        let resolved = resolve_custom_schemas(&loaded, Some(&global), &reporter, &user_agent()).unwrap();

        assert_eq!(resolved.by_type.get(&exoplanet()), Some(&PathBuf::from("/schemas/g.json")));
    }

    #[test]
    fn a_per_input_schema_overrides_the_global_schema() {
        let (config, router) = lane("/maps/planet.json5", "ExoPlanet", Some("/schemas/m.json"));
        let loaded = vec![(&config, router)];
        let reporter = NoopReporter::new();
        let global = SchemaSource::Local(PathBuf::from("/schemas/g.json"));

        let resolved = resolve_custom_schemas(&loaded, Some(&global), &reporter, &user_agent()).unwrap();

        // The per-input schema wins; the global one never reaches this type.
        assert_eq!(resolved.by_type.get(&exoplanet()), Some(&PathBuf::from("/schemas/m.json")));
    }

    #[test]
    fn a_remote_schema_is_downloaded_to_a_temp_file() {
        let port = serve_once("200 OK", r#"{"type":"object","required":["mass"]}"#);
        let (config, router) = lane("/maps/planet.json5", "ExoPlanet", Some(&format!("http://127.0.0.1:{port}/x.json")));
        let loaded = vec![(&config, router)];
        let reporter = NoopReporter::new();

        let resolved = resolve_custom_schemas(&loaded, None, &reporter, &user_agent()).unwrap();

        let schema_path = &resolved.by_type[&exoplanet()];
        assert_eq!(fs::read_to_string(schema_path).unwrap(), r#"{"type":"object","required":["mass"]}"#);
    }

    #[test]
    fn a_failed_remote_fetch_is_fatal() {
        let port = serve_once("404 Not Found", "");
        let (config, router) = lane("/maps/planet.json5", "ExoPlanet", Some(&format!("http://127.0.0.1:{port}/missing.json")));
        let loaded = vec![(&config, router)];
        let reporter = NoopReporter::new();

        let result = resolve_custom_schemas(&loaded, None, &reporter, &user_agent());

        assert!(matches!(result, Err(PipelineError::SchemaFetch { .. })));
    }
}
