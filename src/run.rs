use crate::{
    bug_report,
    cli::{Cli, Commands, config::ConfigSubcommand, manifest::ManifestSubcommand, mapping::MappingSubcommand, schema::SchemaSubcommand, sdm::SdmSubcommand},
    config_overrides::overrides_from_cli,
    error::{Error, Result},
    manifest_build::manifest_from_gen_args,
    pipeline_setup::run_mapping,
    reporter_setup::init_terminal_reporter,
    shutdown,
    worker_threads::worker_thread_count,
};
use cassiopeia_cli::{
    config::CliConfig,
    config_scaffold,
    convert,
    dereference,
    explorer,
    format::{self, FormatOutcome},
    markdown,
    outcome_report::{Outcome, render_outcome},
    profiler,
    schema_listing::{ListingSource, render_schema_listing},
    sdm,
    theme_choice::ThemeChoice,
    wizard,
};
use cassiopeia_configuration::{config::Config, loader::ConfigLoader};
use cassiopeia_directories::locations::config_file;
use cassiopeia_reporter::reporter::Reporter;
use cassiopeia_terminal_style::rendering::{Stream, detect_rendering};
use clap::Parser;
use rayon::ThreadPoolBuilder;
use std::{fs::metadata, path::Path};

/// The pipeline worker threads run deep NGSI-LD resolution recursively, so they need a larger stack
/// than the rayon default.
const WORKER_STACK_SIZE: usize = 8 * 1024 * 1024;

/// Parses the command line, loads configuration, initializes reporting, and dispatches the command.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config(&cli)?;

    shutdown::install_sync_handler()?;
    let reporter = init_terminal_reporter(&config.logger, cli.verbosity())?;
    build_thread_pool(&config)?;

    let cli_config = CliConfig {
        schemas_folder: config.schemas.folder.clone(),
        mappings_folder: config.mappings.folder.clone(),
        download: config.download.clone(),
    };

    dispatch(&cli.command, &config, &cli_config, reporter)
}

/// Loads the configuration, layering the file and the command's CLI overrides.
///
/// Precedence for the file: an explicit `--config` path is honoured verbatim and, being named, is
/// an error when it does not exist. With none given, `config.toml` is auto-discovered in the XDG
/// config directory (or the container config root); its absence is not an error, so it is layered
/// only when it actually exists; otherwise the built-in defaults and the environment stand alone.
fn load_config(cli: &Cli) -> Result<Config> {
    let mut loader = ConfigLoader::new();
    if let Some(path) = &cli.config {
        loader = loader.with_file(path.clone());
    } else {
        let discovered = config_file();
        if discovered.exists() {
            loader = loader.with_file(discovered);
        }
    }
    for config_override in overrides_from_cli(&cli.command) {
        loader = loader.with_override(config_override);
    }

    Ok(loader.load()?)
}

/// Installs the global rayon thread pool the batch pipeline stages run on.
///
/// The pool is sized before any stage starts, so its width is a property of the whole run rather
/// than something an individual stage negotiates.
fn build_thread_pool(config: &Config) -> Result<()> {
    ThreadPoolBuilder::new()
        .num_threads(worker_thread_count(config.pipeline.worker_threads))
        .stack_size(WORKER_STACK_SIZE)
        .build_global()
        .map_err(|source| Error::ThreadPool { source })
}

/// Dispatches a parsed command to its handler.
///
/// One-shot commands do their work and render a structured completion report at this boundary (the
/// human-facing confirmation to stderr, the machine-consumable listing to stdout), so the handlers
/// themselves stay free of presentation. Only the streaming commands (`map`, `sdm download`) still
/// thread the live reporter.
fn dispatch(command: &Commands, config: &Config, cli_config: &CliConfig, reporter: &'static dyn Reporter) -> Result<()> {
    match command {
        Commands::Sdm(command) => dispatch_sdm(&command.command, cli_config, reporter),
        Commands::Mapping(command) => dispatch_mapping(&command.command),
        Commands::Schema(command) => dispatch_schema(&command.command),
        Commands::Manifest(command) => dispatch_manifest(&command.command),
        Commands::Config(command) => dispatch_config(&command.command),
        Commands::Map(args) => run_mapping(args, config, reporter),
        Commands::Explorer { light } => {
            explorer::launch_explorer(cli_config, ThemeChoice::from_light(*light))?;
            Ok(())
        }
        Commands::Wizard { light } => {
            wizard::launch_wizard(cli_config, ThemeChoice::from_light(*light))?;
            Ok(())
        }
        Commands::Profile { file } => {
            profiler::profile_file(file)?;
            Ok(())
        }
        Commands::Bugreport => {
            bug_report::print_report();
            Ok(())
        }
        Commands::Markdown => {
            let path = markdown::generate_markdown::<Cli>("cli.md")?;
            report_outcome(&Outcome::MarkdownWritten {
                size: written_size(&path),
                path,
            });
            Ok(())
        }
    }
}

/// Renders a completed one-shot command's outcome to stderr, coloured only for an interactive
/// terminal, so a redirected or piped invocation keeps stderr plain.
fn report_outcome(outcome: &Outcome) {
    eprintln!("{}", render_outcome(outcome, detect_rendering(Stream::Stderr)));
}

/// The byte size of a file that was just written, or zero when it cannot be stat-ed: a fallback for
/// the effectively-impossible case of a file becoming unreadable between the write and this call.
fn written_size(path: &Path) -> u64 {
    metadata(path).map(|meta| meta.len()).unwrap_or_default()
}

/// Dispatches an `sdm` subcommand. `download` streams progress through the reporter; `list` and
/// `search` write their names to stdout so a pipe (`cass sdm list | grep …`) sees them.
fn dispatch_sdm(command: &SdmSubcommand, cli_config: &CliConfig, reporter: &'static dyn Reporter) -> Result<()> {
    match command {
        SdmSubcommand::Download => {
            sdm::download_schemas(cli_config, reporter)?;
            Ok(())
        }
        SdmSubcommand::List => {
            let names = sdm::list_schemas(cli_config)?;
            println!("{}", render_schema_listing(ListingSource::All, &names, detect_rendering(Stream::Stdout)));
            Ok(())
        }
        SdmSubcommand::Search { query } => {
            let names = sdm::search_schemas(cli_config, query)?;
            println!(
                "{}",
                render_schema_listing(ListingSource::Query(query), &names, detect_rendering(Stream::Stdout))
            );
            Ok(())
        }
    }
}

/// Dispatches a `mapping` subcommand.
fn dispatch_mapping(command: &MappingSubcommand) -> Result<()> {
    match command {
        MappingSubcommand::Format { mapping, replace } => {
            match format::format_mapping(mapping, *replace)? {
                FormatOutcome::InPlace(path) => report_outcome(&Outcome::MappingFormatted { path }),
                // The formatted JSON5 already went to stdout; it is the result, and needs no report.
                FormatOutcome::ToStdout => {}
            }
            Ok(())
        }
        MappingSubcommand::Convert { mapping, output } => {
            let path = convert::convert_mapping(mapping, output)?;
            report_outcome(&Outcome::MappingConverted {
                size: written_size(&path),
                path,
            });
            Ok(())
        }
    }
}

/// Dispatches a `schema` subcommand.
fn dispatch_schema(command: &SchemaSubcommand) -> Result<()> {
    match command {
        SchemaSubcommand::Dereference { schema } => {
            dereference::dereference_schema(schema)?;
            // The dereferenced payload is already on stdout; the outcome owns its path, so the
            // borrowed argument is cloned once for the terminal-final confirmation.
            report_outcome(&Outcome::SchemaDereferenced { schema: schema.clone() });
            Ok(())
        }
    }
}

/// Dispatches a `config` subcommand.
fn dispatch_config(command: &ConfigSubcommand) -> Result<()> {
    match command {
        ConfigSubcommand::Generate(args) => {
            let output = args.output.clone().unwrap_or_else(config_file);
            let path = config_scaffold::write_default_config(&output, args.overwrite())?;
            report_outcome(&Outcome::ConfigurationWritten {
                size: written_size(&path),
                path,
            });
            Ok(())
        }
    }
}

/// Dispatches a `manifest` subcommand.
fn dispatch_manifest(command: &ManifestSubcommand) -> Result<()> {
    match command {
        ManifestSubcommand::Generate(args) => {
            let manifest = manifest_from_gen_args(args)?;
            manifest.write_to_file(&args.manifest_output)?;
            // The outcome owns its path; the write borrows it, so it is cloned once for the report.
            report_outcome(&Outcome::ManifestWritten {
                size: written_size(&args.manifest_output),
                path: args.manifest_output.clone(),
            });
            Ok(())
        }
    }
}
