mod bug_report;
mod build_info;
mod cli;
mod config_overrides;
mod error;
mod failure_code;
mod manifest_build;
mod pipeline_config;
mod pipeline_setup;
mod reporter_setup;
mod run;
mod schedule_build;
mod shutdown;
mod worker_threads;

use crate::failure_code::failure_code;
use cassiopeia_reporter::error_report::report_error;
use human_panic::{Metadata, setup_panic};
use mimalloc::MiMalloc;
use std::process;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() {
    setup_panic!(
        Metadata::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
            .authors("Vela Context <info@velacontext.com>")
            .homepage("https://velacontext.com/")
            .repository("https://github.com/vela-tools/cassiopeia")
            .support("- Open an issue at https://github.com/vela-tools/cassiopeia/issues and attach the generated report file, or email info@velacontext.com.")
    );

    if let Err(error) = run::run() {
        report_error(failure_code(&error), &error);
        process::exit(1);
    }
}
