use crate::template::{source_position::SourcePosition, vocabulary::CallableKind};
use lazy_regex::regex_captures;
use std::error::Error;
use tera::ErrorKind;
use thiserror::Error;

/// Tera's own report of why it refused or failed a template, restated in terms of the template as
/// the author wrote it.
///
/// Tera's rendered report names the template by the digest it is registered under (`tpl_<md5>`)
/// and, for a value expression, quotes the rewritten form it was registered as; neither is anything
/// the author wrote. This keeps Tera's message and, where it points into the author's text, the
/// position it points at, and holds Tera's error itself for a caller that needs it. Tera's error is
/// deliberately not this error's `source()`: walking the chain would print the digest after all.
#[derive(Debug, Error)]
#[error("Tera: {message}{}", .position.map_or_else(String::new, |position| format!(" at {position}")))]
pub struct EngineReport {
    /// Tera's message, with the message of every error it wraps appended.
    message: String,
    /// Where in the author's template Tera's report points, when it points into text the author
    /// wrote.
    position: Option<SourcePosition>,
    /// Tera's error as it raised it.
    engine: tera::Error,
}

impl EngineReport {
    /// Restates Tera's refusal to register `source` as written, positioned in `source`.
    #[must_use]
    pub fn located(source: &str, engine: tera::Error) -> EngineReport {
        let (message, place) = read(&engine);
        EngineReport {
            message,
            position: place.map(|(line, column)| SourcePosition::from_engine(source, line, column)),
            engine,
        }
    }

    /// Restates a failure whose position Tera reports against text the author did not write, such as
    /// the rewritten form a value expression is registered as, so it is left unpositioned.
    #[must_use]
    pub fn unlocated(engine: tera::Error) -> EngineReport {
        let (message, _) = read(&engine);
        EngineReport {
            message,
            position: None,
            engine,
        }
    }

    /// Tera's message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Where in the author's template Tera's report points, if it points into the author's text.
    #[must_use]
    pub const fn position(&self) -> Option<SourcePosition> {
        self.position
    }

    /// Tera's error as it raised it.
    #[must_use]
    pub const fn engine(&self) -> &tera::Error {
        &self.engine
    }

    /// The filter, function, or test Tera reports as not registered, if that is what it reports.
    pub(crate) fn unknown_callable(&self) -> Option<(CallableKind, &str)> {
        let (_, kind, name) = regex_captures!(r"^Unknown (filter|function|test) `([^`]+)`", &self.message)?;

        Some((kind.parse().ok()?, name))
    }

    /// Whether Tera reports a value that is not defined in the record, which Tera names `undefined`.
    pub(crate) fn mentions_undefined_value(&self) -> bool {
        self.message.contains("`undefined`")
    }

    /// Whether Tera reports a null value, which Tera names `none`.
    pub(crate) fn mentions_null_value(&self) -> bool {
        self.message.contains("`none`")
    }
}

/// Tera's message and, when it carries one, the 1-based line and 0-based column it points at.
///
/// A syntax or rendering error carries both as data. Every other report arrives as text: in
/// particular a template naming an unregistered filter, function, or test is reported as Tera's
/// fully rendered report (`error: <message>`, then ` --> <name>:<line>:<column>` with a 1-based
/// column), of which only the first is kept.
fn read(engine: &tera::Error) -> (String, Option<(usize, usize)>) {
    // Tera's error kinds are non-exhaustive, so the two kinds that carry a report are picked out
    // rather than matched exhaustively.
    let (mut message, place) = if let ErrorKind::SyntaxError(report) | ErrorKind::RenderingError(report) = engine.kind() {
        (report.message().to_string(), Some((report.span().start_line, report.span().start_col)))
    } else if let ErrorKind::Msg(text) = engine.kind() {
        read_rendered(text)
    } else {
        (engine.kind().to_string(), None)
    };

    let mut cause = engine.source();
    while let Some(inner) = cause {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        cause = inner.source();
    }

    (message, place)
}

/// The message and position of the first report in Tera's rendered `text`, or the text's first line
/// when it is not a rendered report.
fn read_rendered(text: &str) -> (String, Option<(usize, usize)>) {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or_default();
    let Some(message) = first.strip_prefix("error: ") else {
        return (first.to_string(), None);
    };
    let place = lines.next().and_then(|locus| {
        let (_, line, column) = regex_captures!(r"^\s*--> .*:(\d+):(\d+)$", locus)?;
        let column: usize = column.parse().ok()?;
        Some((line.parse().ok()?, column.checked_sub(1)?))
    });

    (message.to_string(), place)
}

#[cfg(test)]
mod tests {
    use crate::template::{engine_report::EngineReport, vocabulary::CallableKind};
    use std::io;
    use tera::Tera;

    /// Tera's refusal to register `source`.
    fn refusal(source: &str) -> tera::Error {
        Tera::default().add_raw_template("tpl_digest", source).unwrap_err()
    }

    #[test]
    fn a_syntax_error_is_restated_at_its_column_in_the_authors_template() {
        let report = EngineReport::located("{{ a b }}", refusal("{{ a b }}"));

        assert!(report.message().starts_with("Found identifier but expected"), "{}", report.message());
        assert_eq!(report.position().map(|position| position.column()), Some(6));
        assert!(!report.to_string().contains("tpl_digest"), "{report}");
        assert!(report.to_string().starts_with("Tera: "), "{report}");
        assert!(report.to_string().ends_with(" at column 6"), "{report}");
    }

    #[test]
    fn an_unexpected_end_of_input_is_restated_at_the_end_of_the_template() {
        let report = EngineReport::located("{{ t | upper ", refusal("{{ t | upper "));

        assert_eq!(report.to_string(), "Tera: Unexpected end of input at column 13");
    }

    #[test]
    fn an_unknown_filter_is_read_from_teras_rendered_report() {
        let report = EngineReport::located("{{ t | uper }}", refusal("{{ t | uper }}"));

        assert_eq!(report.to_string(), "Tera: Unknown filter `uper` at column 8");
        assert_eq!(report.unknown_callable(), Some((CallableKind::Filter, "uper")));
    }

    #[test]
    fn an_unknown_function_or_test_is_recognised() {
        let function = EngineReport::located("{{ nope() }}", refusal("{{ nope() }}"));
        let test = EngineReport::located("{% if a is nope %}x{% endif %}", refusal("{% if a is nope %}x{% endif %}"));

        assert_eq!(function.unknown_callable(), Some((CallableKind::Function, "nope")));
        assert_eq!(test.unknown_callable(), Some((CallableKind::Test, "nope")));
    }

    #[test]
    fn an_unlocated_report_keeps_only_the_message() {
        let report = EngineReport::unlocated(tera::Error::message("boom"));

        assert_eq!(report.to_string(), "Tera: boom");
        assert_eq!(report.position(), None);
    }

    #[test]
    fn a_wrapped_cause_is_appended_to_the_message() {
        let report = EngineReport::unlocated(tera::Error::chain("Filter `x` failed", io::Error::other("disk gone")));

        assert_eq!(report.message(), "Filter `x` failed: disk gone");
    }

    #[test]
    fn undefined_and_null_values_are_recognised_in_the_message() {
        let undefined = EngineReport::unlocated(tera::Error::message("`+` requires both operands to be numbers, found `undefined` and `i64`"));
        let null = EngineReport::unlocated(tera::Error::message("Invalid type for the value, expected `&str` but got `none`"));

        assert!(undefined.mentions_undefined_value() && !undefined.mentions_null_value());
        assert!(null.mentions_null_value() && !null.mentions_undefined_value());
    }
}
