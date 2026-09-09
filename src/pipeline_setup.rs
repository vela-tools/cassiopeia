use crate::{
    build_info::default_user_agent,
    cli::map::MapArgs,
    error::Result,
    manifest_build::manifest_from_map_args,
    pipeline_config::pipeline_config,
    shutdown,
};
use cassiopeia_common::run_var::run_vars_to_map;
use cassiopeia_configuration::config::Config;
use cassiopeia_manifest::manifest::Manifest;
use cassiopeia_pipeline::{
    observer::{NoopObserver, RunObserver},
    pipeline::{Pipeline, ValidationOverrides},
};
use cassiopeia_reporter::reporter::Reporter;
use std::sync::Arc;

/// Runs the `map` command: builds or loads the manifest, then drives the pipeline to completion.
pub fn run_mapping(args: &MapArgs, config: &Config, reporter: &'static dyn Reporter) -> Result<()> {
    args.validate()?;

    let manifest = load_or_build_manifest(args)?;
    let mut pipeline_config = pipeline_config(config, default_user_agent());
    pipeline_config.mode = args.mode;

    // All inline-only, so they are `None` under `--manifest`, where the manifest's own `output`
    // supplies the equivalents instead.
    let validation = ValidationOverrides {
        report_path: args.validator.report.clone(),
        mode: args.validator.validation_mode,
        schema: args.validator.schema.clone(),
        representation: args.validator.representation,
        skip_null: args.validator.skip_null,
    };
    let observer: Arc<dyn RunObserver> = Arc::new(NoopObserver);

    // The command line's `--var` values override the manifest's global `vars`; a per-input `vars`
    // still wins over both, resolved inside `from_manifest`.
    let cli_vars = run_vars_to_map(&args.var);

    let mut pipeline = Pipeline::from_manifest(&manifest, validation, cli_vars, pipeline_config, reporter, shutdown::flag(), observer)?;

    pipeline.run()?;

    Ok(())
}

/// Loads the manifest from `--manifest` when given, otherwise builds one from the input arguments.
fn load_or_build_manifest(args: &MapArgs) -> Result<Manifest> {
    match &args.manifest {
        Some(path) => Ok(Manifest::from_file(path)?),
        None => manifest_from_map_args(args),
    }
}
