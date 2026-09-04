use crate::cli::version::plain_version_banner;
use bugreport::{
    CrateInfo,
    collector::{CollectionError, Collector},
    report::{Code, ReportEntry},
};

/// A [`Collector`] emitting Cassiopeia's build provenance: the same four lines `--version` prints,
/// stripped of styling.
///
/// This is the single most useful section of a Cassiopeia bug report. It pins the exact source
/// revision (commit and clean/dirty tree state) and, through the feature flags, records which
/// optional code paths were compiled into the binary at hand, most consequentially the two GRIB
/// collectors that link the native ecCodes library, which a build can legitimately ship without.
pub struct BuildProvenance;

impl Collector for BuildProvenance {
    fn description(&self) -> &'static str {
        "Build provenance"
    }

    /// Renders the banner as a fenced code block so its four lines survive verbatim in Markdown,
    /// where a plain-text entry's line breaks would be collapsed into one paragraph. The report is
    /// assembled purely from compile-time constants, so collection is infallible.
    fn collect(&mut self, _crate_info: &CrateInfo<'_>) -> Result<ReportEntry, CollectionError> {
        Ok(ReportEntry::Code(Code {
            language: None,
            code: plain_version_banner(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::bug_report::build_provenance::BuildProvenance;
    use bugreport::{bugreport, format::Markdown};

    #[test]
    fn the_provenance_section_pins_the_release_version() {
        let report = bugreport!().info(BuildProvenance).format::<Markdown>();

        assert!(report.contains("#### Build provenance"));
        assert!(report.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn the_provenance_section_is_a_fenced_code_block() {
        let report = bugreport!().info(BuildProvenance).format::<Markdown>();

        assert!(report.contains("```"));
    }
}
