use crate::build_info::build;
use cassiopeia_terminal_style::{
    paint::{join, paint},
    palette::{ACCENT, CAUTION, FRAME, HIGHLIGHT, LABEL, MUTED, PRIMARY, SUCCESS},
    rendering::{Rendering, Stream, detect_rendering},
};
use std::sync::LazyLock;

/// The one-line summary shown under `--help` and next to the command in a parent listing.
pub const ABOUT: &str = "Map and transform source data into standards-compliant NGSI-LD entities.";

/// The edition label stamped into this binary's version banner. This crate is the open-source
/// build; the proprietary superset ships as a separate binary that supplies its own banner.
const EDITION: &str = "foss";

/// The licence this build is distributed under, read from the crate manifest (EUPL-1.2).
const LICENSE: &str = env!("CARGO_PKG_LICENSE");

/// Whether an optional, compile-time capability was built into this binary. Reported in the version
/// banner so an operator can tell, from the banner alone, whether a feature-gated code path is
/// reachable in the binary they are holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Capability {
    Present,
    Absent,
}

/// The optional, feature-gated capabilities this build may carry, each paired with whether it was
/// compiled in. Both GRIB features link the native ecCodes library, so a build can legitimately ship
/// without either; the banner has to make that difference visible. `grib1` routes GRIB1 through
/// ecCodes; `grib2-full` routes GRIB2 through ecCodes too (projected grids and more parameter tables),
/// versus the always-present pure-Rust GRIB2 fallback.
const fn capabilities() -> [(&'static str, Capability); 2] {
    let grib1 = if cfg!(feature = "grib1") { Capability::Present } else { Capability::Absent };
    let grib2_full = if cfg!(feature = "grib2-full") {
        Capability::Present
    } else {
        Capability::Absent
    };
    [("grib1", grib1), ("grib2-full", grib2_full)]
}

/// Renders the feature-flag line: each optional capability marked `+name` when compiled in or
/// `-name` when absent, coloured green for present and amber for absent to match the tree-state cue.
fn render_capabilities(rendering: Rendering) -> String {
    let flags = capabilities()
        .iter()
        .map(|(name, support)| {
            let (mark, style) = match support {
                Capability::Present => ("+", SUCCESS),
                Capability::Absent => ("-", CAUTION),
            };
            paint(style, &format!("{mark}{name}"), rendering)
        })
        .collect::<Vec<_>>();
    format!("{}{}", paint(LABEL, "features ", rendering), join(&flags, rendering))
}

/// Builds the four-line `--version` block: identity, provenance, feature flags, then build
/// environment.
///
/// Kept pure and parameterised over [`Rendering`] so both the coloured and plain forms are testable
/// without touching the terminal. The binary name is not repeated here: clap prepends it.
fn render_long_version(rendering: Rendering) -> String {
    let identity = join(
        &[
            paint(PRIMARY, build::PKG_VERSION, rendering),
            paint(HIGHLIGHT, &format!("{EDITION} edition"), rendering),
        ],
        rendering,
    );

    let mut provenance = Vec::new();
    let commit = build::SHORT_COMMIT;
    if !commit.is_empty() {
        provenance.push(paint(ACCENT, commit, rendering));
        let (label, style) = if build::GIT_CLEAN { ("clean", SUCCESS) } else { ("dirty", CAUTION) };
        provenance.push(paint(style, label, rendering));
    }
    provenance.push(paint(HIGHLIGHT, LICENSE, rendering));
    let provenance = join(&provenance, rendering);

    // shadow-rs stamps a full timestamp, an rustc version string with a commit suffix, and a target
    // triple as the rustc channel; the version line wants only the leading, human-scannable part of
    // each, so the trailing detail is trimmed off here.
    let date = build::BUILD_TIME.split_whitespace().next().unwrap_or(build::BUILD_TIME);
    let rustc = build::RUST_VERSION.split(" (").next().unwrap_or(build::RUST_VERSION);
    let channel = build::RUST_CHANNEL.split('-').next().unwrap_or(build::RUST_CHANNEL);
    let environment = join(
        &[
            paint(MUTED, date, rendering),
            paint(MUTED, &format!("{rustc} {channel}"), rendering),
            paint(MUTED, build::BUILD_TARGET, rendering),
        ],
        rendering,
    );
    let environment = format!("{}{environment}", paint(FRAME, "built ", rendering));

    let capabilities = render_capabilities(rendering);

    format!("{identity}\n{provenance}\n{capabilities}\n{environment}")
}

static LONG_VERSION: LazyLock<String> = LazyLock::new(|| render_long_version(detect_rendering(Stream::Stdout)));

/// The terse `-V` string: the bare release number, always plain for scripts and version filters.
pub const fn short_version() -> &'static str {
    build::PKG_VERSION
}

/// The rich `--version` block, coloured for a terminal and plain otherwise. Built once and cached.
pub fn long_version() -> &'static str {
    LONG_VERSION.as_str()
}

/// The same four-line provenance block as `--version`, rendered without any styling.
///
/// A bug report embeds this verbatim: ANSI escapes would be noise in pasted output, and the plain
/// form still carries every diagnostic detail (release, edition, commit, tree state, licence,
/// compiled-in features, and build environment). Recomputed on demand rather than reusing the cached
/// coloured banner, since that one is styled for the terminal.
pub fn plain_version_banner() -> String {
    render_long_version(Rendering::Plain)
}

#[cfg(test)]
mod tests {
    use crate::cli::version::{LICENSE, long_version, render_long_version, short_version};
    use cassiopeia_terminal_style::rendering::Rendering;

    const ESCAPE: char = '\u{1b}';

    #[test]
    fn the_license_is_the_manifest_licence() {
        assert_eq!(LICENSE, env!("CARGO_PKG_LICENSE"));
    }

    #[test]
    fn the_short_version_is_the_bare_release_number() {
        assert_eq!(short_version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn the_plain_long_version_carries_identity_provenance_and_environment() {
        let block = render_long_version(Rendering::Plain);

        assert!(block.contains(env!("CARGO_PKG_VERSION")));
        assert!(block.contains("edition"));
        assert!(block.contains(LICENSE));
        assert!(block.contains("built "));
        assert_eq!(block.lines().count(), 4);
    }

    #[test]
    fn the_plain_long_version_reports_the_grib1_capability_with_its_build_state() {
        let block = render_long_version(Rendering::Plain);
        let expected = if cfg!(feature = "grib1") { "+grib1" } else { "-grib1" };

        assert!(block.contains("features "));
        assert!(block.contains(expected));
    }

    #[test]
    fn the_plain_long_version_reports_the_grib2_full_capability_with_its_build_state() {
        let block = render_long_version(Rendering::Plain);
        let expected = if cfg!(feature = "grib2-full") { "+grib2-full" } else { "-grib2-full" };

        assert!(block.contains("features "));
        assert!(block.contains(expected));
    }

    #[test]
    fn the_plain_long_version_has_no_escape_codes() {
        assert!(!render_long_version(Rendering::Plain).contains(ESCAPE));
    }

    #[test]
    fn the_coloured_long_version_carries_escape_codes() {
        assert!(render_long_version(Rendering::Colored).contains(ESCAPE));
    }

    #[test]
    fn the_cached_long_version_is_non_empty() {
        assert!(!long_version().is_empty());
    }
}
