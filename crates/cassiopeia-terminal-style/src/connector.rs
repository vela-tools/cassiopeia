//! The box-drawing connectors that hang detail lines under a headline.

/// The connector for a detail line that is followed by another.
pub const BRANCH: &str = "├─";

/// The connector for the last detail line under a headline.
pub const LAST_BRANCH: &str = "╰─";

/// The indent every detail line sits at, so the connectors align under the headline's glyph.
pub const DETAIL_INDENT: &str = "  ";

/// The connector for the line at `index` of a run of `total` detail lines.
#[must_use]
pub const fn connector(index: usize, total: usize) -> &'static str {
    if index + 1 == total { LAST_BRANCH } else { BRANCH }
}

#[cfg(test)]
mod tests {
    use crate::connector::{BRANCH, LAST_BRANCH, connector};

    #[test]
    fn the_last_line_of_a_run_closes_the_branch() {
        assert_eq!(connector(0, 3), BRANCH);
        assert_eq!(connector(1, 3), BRANCH);
        assert_eq!(connector(2, 3), LAST_BRANCH);
    }

    #[test]
    fn a_single_detail_line_closes_immediately() {
        assert_eq!(connector(0, 1), LAST_BRANCH);
    }
}
