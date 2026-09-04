//! The `bugreport` command: a diagnostic report a user prints and pastes when filing an issue.
//!
//! The report answers the three questions a maintainer asks first. *What binary is this?*: the
//! release, exact commit, and which optional features (the ecCodes-linked GRIB paths) were compiled
//! in. *What ran it?*: the host operating system and the exact, shell-escaped command line. *Where
//! did it look?*: the configuration file, schema store, and mapping folder a run reads from, so a
//! "not found" report shows at a glance whether those inputs are where Cassiopeia expected them.
//!
//! Nothing here reads a file's contents or names individual entries: the configuration file may carry
//! broker credentials and the logs may carry ingested data. Only path existence and coarse entry
//! counts are collected. The output still passes through the user's own eyes before it reaches an
//! issue tracker.

use crate::bug_report::{build_provenance::BuildProvenance, run_locations::RunLocations};
use bugreport::{
    bugreport,
    collector::{CommandLine, CompileTimeInformation, EnvironmentVariables, OperatingSystem, SoftwareVersion},
    format::Markdown,
};

mod build_provenance;
mod run_locations;

/// The environment variables worth reporting: those that decide where the XDG-based locations
/// resolve, and those that shape logging and terminal rendering. None carries a secret, and each
/// explains behaviour a maintainer would otherwise have to guess at: an unexpected config path, a
/// missing colour, a backtrace that was or was not requested.
const REPORTED_ENVIRONMENT: [&str; 7] = [
    "RUST_BACKTRACE",
    "RUST_LOG",
    "NO_COLOR",
    "TERM",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
];

/// Assembles the diagnostic report as Markdown.
///
/// Ordered for a maintainer reading top to bottom: identity and build provenance, then the host and
/// how the tool was invoked, then the locations a run depends on and the listings of what they hold.
fn render_report() -> String {
    bugreport!()
        .info(SoftwareVersion::default())
        .info(BuildProvenance)
        .info(CompileTimeInformation::default())
        .info(OperatingSystem::default())
        .info(CommandLine::default())
        .info(EnvironmentVariables::list(&REPORTED_ENVIRONMENT))
        .info(RunLocations)
        .format::<Markdown>()
}

/// Prints the diagnostic report to stdout, where it can be piped or pasted directly into an issue.
pub fn print_report() {
    println!("{}", render_report());
}

#[cfg(test)]
mod tests {
    use crate::bug_report::render_report;

    #[test]
    fn the_report_carries_every_diagnostic_section() {
        let report = render_report();

        for title in [
            "#### Software version",
            "#### Build provenance",
            "#### Compile time information",
            "#### Operating system",
            "#### Command-line",
            "#### Environment variables",
            "#### Run locations",
        ] {
            assert!(report.contains(title), "the report is missing the {title} section");
        }
    }
}
