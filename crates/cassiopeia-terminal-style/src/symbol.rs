//! Status glyphs prefixed to a message line to signal its severity at a glance.

/// The glyph marking an informational line.
pub const INFO_SYMBOL: &str = "•";
/// The glyph marking a warning line.
pub const WARN_SYMBOL: &str = "!";
/// The glyph marking an error line.
pub const ERROR_SYMBOL: &str = "×";
/// The glyph marking a success line.
pub const SUCCESS_SYMBOL: &str = "✓";

#[cfg(test)]
mod tests {
    use crate::symbol::{ERROR_SYMBOL, INFO_SYMBOL, SUCCESS_SYMBOL, WARN_SYMBOL};
    use std::collections::HashSet;

    #[test]
    fn every_status_symbol_is_a_single_distinct_glyph() {
        let symbols = [INFO_SYMBOL, WARN_SYMBOL, ERROR_SYMBOL, SUCCESS_SYMBOL];
        for symbol in symbols {
            assert_eq!(symbol.chars().count(), 1);
        }
        assert_eq!(symbols.iter().collect::<HashSet<_>>().len(), 4);
    }
}
