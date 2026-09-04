//! Rendering one [`Diagnostic`] as terminal lines.
//!
//! The function is pure: a diagnostic, a verbosity, and a rendering in, ready-to-print lines out.
//! Both forms can be asserted exactly without a terminal, and the policy for what shows at
//! which level lives in one readable place rather than spread through the backend.

use anstyle::Style;
use cassiopeia_common::captured_body::{BodyCapture, CapturedBody};
use cassiopeia_diagnostic::{context_field::ContextField, diagnostic::Diagnostic, severity::Severity, verbosity::Verbosity};
use cassiopeia_terminal_style::{
    byte_size::human_size,
    connector::{DETAIL_INDENT, connector},
    paint::{join, paint},
    palette::{CAUTION, ERROR, FRAME},
    rendering::Rendering,
    symbol::{ERROR_SYMBOL, WARN_SYMBOL},
};

/// The widest label the detail rows align to, chosen so the longest label still leaves a gap.
const LABEL_WIDTH: usize = 12;

/// How many detail lines the concise form allows under a headline.
const CONCISE_DETAIL_LINES: usize = 2;

/// Renders a diagnostic as the lines to print, headline first.
///
/// The concise form is a headline plus at most two unlabelled detail lines: what a reader needs to
/// know without being told everything. The verbose form drops the headline's qualifier (it becomes
/// a row of its own) and lists the cause chain ahead of every context field.
pub(crate) fn diagnostic_lines(diagnostic: &Diagnostic, verbosity: Verbosity, rendering: Rendering) -> Vec<String> {
    let (headline, details) = match verbosity {
        Verbosity::Concise => (concise_headline(diagnostic, rendering), concise_details(diagnostic)),
        Verbosity::Full => (plain_headline(diagnostic, rendering), full_details(diagnostic)),
    };

    let mut lines = Vec::with_capacity(details.len() + 1);
    lines.push(headline);
    for (index, detail) in details.iter().enumerate() {
        lines.push(format!("{DETAIL_INDENT}{} {detail}", paint(FRAME, connector(index, details.len()), rendering)));
    }
    lines
}

/// The glyph a severity is prefixed with.
const fn symbol_for(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => ERROR_SYMBOL,
        Severity::Warning => WARN_SYMBOL,
    }
}

/// The colour a severity's glyph carries.
const fn style_for(severity: Severity) -> Style {
    match severity {
        Severity::Error => ERROR,
        Severity::Warning => CAUTION,
    }
}

/// The headline with only its glyph coloured, so the message itself stays readable on any terminal.
fn plain_headline(diagnostic: &Diagnostic, rendering: Rendering) -> String {
    let severity = diagnostic.severity();
    format!("{} {}", paint(style_for(severity), symbol_for(severity), rendering), diagnostic.headline())
}

/// The concise headline, with the qualifier the verbose form would list as a row appended after the
/// shared middot separator.
fn concise_headline(diagnostic: &Diagnostic, rendering: Rendering) -> String {
    let severity = diagnostic.severity();
    let mut segments = vec![diagnostic.headline().to_owned()];
    segments.extend(diagnostic.defining().iter().filter_map(qualifier));

    format!("{} {}", paint(style_for(severity), symbol_for(severity), rendering), join(&segments, rendering))
}

/// The value a field contributes to the concise headline after the middot, if any.
///
/// Only a status qualifies: it is the one fact short enough to sit on the headline and specific
/// enough to be worth putting there. Matching every variant means adding a field forces this policy
/// to be revisited, which is what keeps presentation out of the diagnostic vocabulary.
fn qualifier(field: &ContextField) -> Option<String> {
    match field {
        ContextField::HttpStatus(status) => Some(status.as_u16().to_string()),
        ContextField::Endpoint(_)
        | ContextField::ProblemType(_)
        | ContextField::EntityType(_)
        | ContextField::Attribute(_)
        | ContextField::InstancePath(_)
        | ContextField::SchemaPath(_)
        | ContextField::Keyword(_)
        | ContextField::Detail(_)
        | ContextField::Entities { .. }
        | ContextField::SourcePath(_)
        | ContextField::Attempt { .. }
        | ContextField::PayloadBytes(_)
        | ContextField::BodyEcho(_)
        | ContextField::RegistrationId(_)
        | ContextField::BatchSize(_) => None,
    }
}

/// The peer's own explanation, if this field is one.
const fn explanation(field: &ContextField) -> Option<&str> {
    match field {
        ContextField::Detail(detail) => Some(detail.as_str()),
        ContextField::HttpStatus(_)
        | ContextField::Endpoint(_)
        | ContextField::ProblemType(_)
        | ContextField::EntityType(_)
        | ContextField::Attribute(_)
        | ContextField::InstancePath(_)
        | ContextField::SchemaPath(_)
        | ContextField::Keyword(_)
        | ContextField::Entities { .. }
        | ContextField::SourcePath(_)
        | ContextField::Attempt { .. }
        | ContextField::PayloadBytes(_)
        | ContextField::BodyEcho(_)
        | ContextField::RegistrationId(_)
        | ContextField::BatchSize(_) => None,
    }
}

/// The concise form's detail lines: the root cause and the peer's explanation, capped so the default
/// view can never grow past a headline plus two lines however much context a diagnostic carries.
fn concise_details(diagnostic: &Diagnostic) -> Vec<String> {
    let mut details: Vec<String> = Vec::with_capacity(CONCISE_DETAIL_LINES);
    if let Some(cause) = diagnostic.causes().first() {
        details.push(cause.as_str().to_owned());
    }
    if let Some(detail) = diagnostic.defining().iter().find_map(explanation)
        && !details.iter().any(|existing| existing == detail)
    {
        details.push(detail.to_owned());
    }
    details.truncate(CONCISE_DETAIL_LINES);
    details
}

/// The verbose form's detail rows: the cause chain first, then every context field, defining before
/// incidental.
fn full_details(diagnostic: &Diagnostic) -> Vec<String> {
    let causes = diagnostic.causes().iter().map(|cause| detail_row("caused by", cause.as_str()));
    let fields = diagnostic.defining().iter().chain(diagnostic.incidental()).map(|field| {
        let (label, value) = field_row(field);
        detail_row(label, &value)
    });
    causes.chain(fields).collect()
}

/// One label-aligned row.
fn detail_row(label: &str, value: &str) -> String {
    format!("{label:<LABEL_WIDTH$}{value}")
}

/// The label and rendered value of one context field.
fn field_row(field: &ContextField) -> (&'static str, String) {
    match field {
        ContextField::HttpStatus(status) => (
            "status",
            match status.canonical_reason() {
                Some(reason) => format!("{} {reason}", status.as_u16()),
                None => status.as_u16().to_string(),
            },
        ),
        ContextField::Endpoint(url) => ("endpoint", url.as_str().to_owned()),
        ContextField::ProblemType(problem_type) => ("type", problem_type.as_str().to_owned()),
        ContextField::EntityType(entity_type) => ("entity type", entity_type.to_string()),
        ContextField::Attribute(attribute) => ("attribute", attribute.to_string()),
        ContextField::InstancePath(pointer) => ("instance", pointer.as_str().to_owned()),
        ContextField::SchemaPath(pointer) => ("schema", pointer.as_str().to_owned()),
        ContextField::Keyword(keyword) => ("keyword", keyword.to_string()),
        ContextField::Detail(detail) => ("detail", detail.as_str().to_owned()),
        ContextField::Entities { first, additional } => (
            "entities",
            if *additional == 0 {
                first.as_str().to_owned()
            } else {
                format!("{} (+{additional})", first.as_str())
            },
        ),
        ContextField::SourcePath(path) => ("source", path.display().to_string()),
        ContextField::Attempt { attempt, limit } => ("attempt", format!("{attempt}/{limit}")),
        ContextField::PayloadBytes(bytes) => ("payload", human_size(*bytes)),
        ContextField::BodyEcho(body) => ("body", body_value(body)),
        ContextField::RegistrationId(registration) => ("registered", registration.as_str().to_owned()),
        ContextField::BatchSize(size) => ("batch", size.to_string()),
    }
}

/// A captured body squashed onto one line, saying so when it is partial or was never readable.
fn body_value(body: &CapturedBody) -> String {
    let text = body.text();
    let squashed: Vec<&str> = text.split_whitespace().collect();
    let squashed = squashed.join(" ");
    match body.capture() {
        BodyCapture::Complete => squashed,
        BodyCapture::Truncated { total } => format!("{squashed} … of {}", human_size(u64::try_from(*total).unwrap_or(u64::MAX))),
        BodyCapture::Unreadable { reason } => format!("unreadable: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use crate::backend::terminal::diagnostic_lines::diagnostic_lines;
    use cassiopeia_diagnostic::{
        code::{broker_code::BrokerCode, diagnostic_code::DiagnosticCode, schema_code::SchemaCode},
        context_field::ContextField,
        detail::Detail,
        diagnostic::Diagnostic,
        diagnostic_builder::DiagnosticBuilder,
        severity::Severity,
        verbosity::Verbosity,
    };
    use cassiopeia_terminal_style::rendering::Rendering;
    use http::StatusCode;
    use iri_rs::IriBuf;
    use url::Url;

    const ESCAPE: char = '\u{1b}';

    /// The broker rejection the approved output is written against.
    fn rejection() -> Diagnostic {
        DiagnosticBuilder::new(
            Severity::Error,
            DiagnosticCode::Broker(BrokerCode::BatchRejected),
            "Broker rejected 100 entities",
        )
        .with_context(ContextField::HttpStatus(StatusCode::UNPROCESSABLE_ENTITY))
        .with_context(ContextField::Endpoint(Url::parse("https://broker/ngsi-ld/v1/entityOperations/upsert").unwrap()))
        .with_context(ContextField::ProblemType("https://uri.etsi.org/ngsi-ld/errors/BadRequestData".parse().unwrap()))
        .with_context(ContextField::Entities {
            first: IriBuf::new("urn:ngsi-ld:AirQualityObserved:LJ-001".to_owned()).unwrap(),
            additional: 99,
        })
        .with_context(ContextField::Detail(Detail::new("attribute 'dateObserved' is not a valid DateTime")))
        .build()
    }

    #[test]
    fn the_concise_form_is_a_headline_and_one_explanation() {
        assert_eq!(
            diagnostic_lines(&rejection(), Verbosity::Concise, Rendering::Plain),
            vec![
                "\u{d7} Broker rejected 100 entities \u{b7} 422".to_owned(),
                "  \u{2570}\u{2500} attribute 'dateObserved' is not a valid DateTime".to_owned(),
            ]
        );
    }

    #[test]
    fn the_verbose_form_lists_every_field_and_drops_the_qualifier() {
        assert_eq!(
            diagnostic_lines(&rejection(), Verbosity::Full, Rendering::Plain),
            vec![
                "\u{d7} Broker rejected 100 entities".to_owned(),
                "  \u{251c}\u{2500} status      422 Unprocessable Entity".to_owned(),
                "  \u{251c}\u{2500} endpoint    https://broker/ngsi-ld/v1/entityOperations/upsert".to_owned(),
                "  \u{251c}\u{2500} type        https://uri.etsi.org/ngsi-ld/errors/BadRequestData".to_owned(),
                "  \u{251c}\u{2500} detail      attribute 'dateObserved' is not a valid DateTime".to_owned(),
                "  \u{2570}\u{2500} entities    urn:ngsi-ld:AirQualityObserved:LJ-001 (+99)".to_owned(),
            ]
        );
    }

    #[test]
    fn a_cause_chain_renders_as_labelled_rows_ahead_of_the_context() {
        let diagnostic = DiagnosticBuilder::new(
            Severity::Error,
            DiagnosticCode::Broker(BrokerCode::BatchUnreadable),
            "Broker 207 body could not be read",
        )
        .with_cause("expected value at line 1 column 1")
        .with_context(ContextField::HttpStatus(StatusCode::MULTI_STATUS))
        .build();

        let lines = diagnostic_lines(&diagnostic, Verbosity::Full, Rendering::Plain);

        assert_eq!(lines[1], "  \u{251c}\u{2500} caused by   expected value at line 1 column 1");
        assert_eq!(lines[2], "  \u{2570}\u{2500} status      207 Multi-Status");
    }

    #[test]
    fn a_diagnostic_without_a_cause_or_detail_is_a_single_line() {
        let diagnostic = DiagnosticBuilder::new(
            Severity::Error,
            DiagnosticCode::Broker(BrokerCode::WorkerPanicked),
            "Broker worker thread panicked",
        )
        .build();

        assert_eq!(
            diagnostic_lines(&diagnostic, Verbosity::Concise, Rendering::Plain),
            vec!["\u{d7} Broker worker thread panicked".to_owned()]
        );
    }

    #[test]
    fn a_warning_carries_the_caution_glyph() {
        let diagnostic = DiagnosticBuilder::new(
            Severity::Warning,
            DiagnosticCode::Schema(SchemaCode::Nonconformant),
            "1482 'AirQualityObserved' entities do not conform to their schema",
        )
        .build();

        assert!(diagnostic_lines(&diagnostic, Verbosity::Concise, Rendering::Plain)[0].starts_with("! "));
    }

    #[test]
    fn plain_output_carries_no_escape_codes() {
        for verbosity in [Verbosity::Concise, Verbosity::Full] {
            for line in diagnostic_lines(&rejection(), verbosity, Rendering::Plain) {
                assert!(!line.contains(ESCAPE));
            }
        }
    }

    #[test]
    fn coloured_output_styles_the_glyph_but_not_the_headline_text() {
        let lines = diagnostic_lines(&rejection(), Verbosity::Full, Rendering::Colored);
        let headline = &lines[0];

        assert!(headline.contains(ESCAPE));
        assert!(headline.ends_with("Broker rejected 100 entities"));
    }

    #[test]
    fn the_concise_form_never_grows_past_a_headline_plus_two_lines() {
        let diagnostic = DiagnosticBuilder::new(
            Severity::Error,
            DiagnosticCode::Broker(BrokerCode::BatchRejected),
            "Broker rejected 100 entities",
        )
        .with_cause("outer")
        .with_cause("inner")
        .with_context(ContextField::Detail(Detail::new("bad date")))
        .with_context(ContextField::HttpStatus(StatusCode::BAD_REQUEST))
        .build();

        assert_eq!(diagnostic_lines(&diagnostic, Verbosity::Concise, Rendering::Plain).len(), 3);
    }
}
