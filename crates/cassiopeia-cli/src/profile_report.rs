use cassiopeia_data_profiler::{
    inspectors::csv::dialect::{Dialect, LineTerminator},
    metadata::{FormatMetadata, GribEdition},
    profile::Profile,
};
use cassiopeia_terminal_style::{
    byte_size::human_size,
    field::field,
    paint::{join, paint},
    palette::{ACCENT, HIGHLIGHT, MUTED, PRIMARY},
    rendering::Rendering,
};

/// Renders a compact, `--version`-style profile report: an identity line (file and size), a format
/// line (format, media type, confidence), and one format-specific metadata line when the profile
/// carries metadata.
///
/// Parameterised over [`Rendering`] so both the coloured and plain forms are testable without a
/// terminal. New formats extend only [`metadata_line`]; the identity and format lines are shared by
/// every format.
#[must_use]
pub fn render_profile(name: &str, size: Option<u64>, profile: &Profile, rendering: Rendering) -> String {
    let mut identity = vec![paint(PRIMARY, name, rendering)];
    if let Some(size) = size {
        identity.push(paint(MUTED, &human_size(size), rendering));
    }

    let confidence = f64::from(*profile.confidence()) * 100.0;
    let format = join(
        &[
            paint(ACCENT, &profile.format().to_string(), rendering),
            paint(MUTED, &profile.mime_type().to_string(), rendering),
            paint(MUTED, &format!("{confidence:.0}% confidence"), rendering),
        ],
        rendering,
    );

    let mut lines = vec![join(&identity, rendering), format];
    if let Some(metadata) = metadata_line(profile.metadata().as_ref(), rendering) {
        lines.push(metadata);
    }
    lines.join("\n")
}

/// Renders the format-specific metadata line, or `None` for a format that carries no metadata.
fn metadata_line(metadata: Option<&FormatMetadata>, rendering: Rendering) -> Option<String> {
    match metadata? {
        FormatMetadata::Csv(csv) => Some(csv_line(csv.dialect(), rendering)),
        FormatMetadata::Grib(grib) => Some(grib_line(grib.edition(), rendering)),
    }
}

/// Renders the CSV dialect line: delimiter, quoting, optional escape, header, terminator, encoding.
fn csv_line(dialect: &Dialect, rendering: Rendering) -> String {
    let mut fields = vec![
        field("delimiter", &byte_token(dialect.delimiter), rendering),
        field("quote", &optional_byte_token(dialect.quote_char), rendering),
    ];
    if let Some(escape) = dialect.escape {
        fields.push(field("escape", &byte_token(escape), rendering));
    }
    fields.push(paint(HIGHLIGHT, header_token(dialect.has_headers), rendering));
    fields.push(paint(MUTED, terminator_token(dialect.terminator), rendering));
    fields.push(paint(MUTED, &dialect.encoding, rendering));
    join(&fields, rendering)
}

/// Renders the GRIB metadata line: the edition read from the Indicator Section.
fn grib_line(edition: GribEdition, rendering: Rendering) -> String {
    let number = match edition {
        GribEdition::V1 => "1",
        GribEdition::V2 => "2",
    };
    field("edition", number, rendering)
}

/// Renders a delimiter, quote, or escape byte as a readable token: the literal character, or a word
/// for a whitespace byte that would otherwise be invisible.
fn byte_token(byte: u8) -> String {
    match byte {
        b'\t' => "tab".to_owned(),
        b' ' => "space".to_owned(),
        other => char::from(other).to_string(),
    }
}

/// Renders an optional byte as its token, or `none` when the dialect disables it.
fn optional_byte_token(byte: Option<u8>) -> String {
    match byte {
        Some(byte) => byte_token(byte),
        None => "none".to_owned(),
    }
}

/// The header token for a dialect.
const fn header_token(has_headers: bool) -> &'static str {
    if has_headers { "header row" } else { "no header" }
}

/// The line-terminator token for a dialect.
const fn terminator_token(terminator: LineTerminator) -> &'static str {
    match terminator {
        LineTerminator::Lf => "LF",
        LineTerminator::CrLf => "CRLF",
    }
}

#[cfg(test)]
mod tests {
    use crate::profile_report::{byte_token, csv_line, grib_line, metadata_line, optional_byte_token, render_profile};
    use cassiopeia_data_profiler::{
        inspectors::csv::dialect::{Dialect, LineTerminator},
        metadata::{CsvMetadata, FormatMetadata, GribEdition, GribMetadata},
        profile_bytes,
    };
    use cassiopeia_terminal_style::rendering::Rendering;

    const ESCAPE: char = '\u{1b}';

    fn comma_dialect() -> Dialect {
        Dialect {
            delimiter: b',',
            quote_char: Some(b'"'),
            has_headers: true,
            escape: None,
            terminator: LineTerminator::Lf,
            encoding: "UTF-8".to_owned(),
        }
    }

    #[test]
    fn a_byte_token_names_whitespace_and_prints_other_bytes() {
        assert_eq!(byte_token(b','), ",");
        assert_eq!(byte_token(b';'), ";");
        assert_eq!(byte_token(b'\t'), "tab");
        assert_eq!(byte_token(b' '), "space");
    }

    #[test]
    fn an_optional_byte_token_reports_none_when_disabled() {
        assert_eq!(optional_byte_token(Some(b'"')), "\"");
        assert_eq!(optional_byte_token(None), "none");
    }

    #[test]
    fn a_csv_line_states_the_dialect() {
        let line = csv_line(&comma_dialect(), Rendering::Plain);

        assert_eq!(line, "delimiter , \u{b7} quote \" \u{b7} header row \u{b7} LF \u{b7} UTF-8");
    }

    #[test]
    fn a_headerless_tab_csv_line_reflects_the_dialect() {
        let dialect = Dialect {
            delimiter: b'\t',
            quote_char: None,
            has_headers: false,
            escape: None,
            terminator: LineTerminator::CrLf,
            encoding: "windows-1252".to_owned(),
        };
        let line = csv_line(&dialect, Rendering::Plain);

        assert_eq!(line, "delimiter tab \u{b7} quote none \u{b7} no header \u{b7} CRLF \u{b7} windows-1252");
    }

    #[test]
    fn a_grib_line_states_the_edition() {
        assert_eq!(grib_line(GribEdition::V1, Rendering::Plain), "edition 1");
        assert_eq!(grib_line(GribEdition::V2, Rendering::Plain), "edition 2");
    }

    #[test]
    fn a_metadata_line_is_absent_for_a_format_without_metadata() {
        assert!(metadata_line(None, Rendering::Plain).is_none());
    }

    #[test]
    fn a_metadata_line_renders_csv_and_grib_variants() {
        let csv = FormatMetadata::Csv(CsvMetadata::new(comma_dialect()));
        let grib = FormatMetadata::Grib(GribMetadata::new(GribEdition::V2));

        assert!(metadata_line(Some(&csv), Rendering::Plain).unwrap().contains("delimiter"));
        assert_eq!(metadata_line(Some(&grib), Rendering::Plain).unwrap(), "edition 2");
    }

    #[test]
    fn the_report_leads_with_identity_then_format() {
        let profile = profile_bytes(b"id,name,value\nA-17,Main,21.5\nA-18,North,20.9\n").unwrap();
        let report = render_profile("energy.csv", Some(133_744_204), &profile, Rendering::Plain);
        let mut lines = report.lines();

        assert_eq!(lines.next().unwrap(), "energy.csv \u{b7} 127.5 MiB");
        let format = lines.next().unwrap();
        assert!(format.starts_with("CSV"));
        assert!(format.contains("% confidence"));
    }

    #[test]
    fn a_report_omits_the_size_when_it_is_unknown() {
        let profile = profile_bytes(b"id,name,value\nA-17,Main,21.5\nA-18,North,20.9\n").unwrap();
        let report = render_profile("energy.csv", None, &profile, Rendering::Plain);

        assert!(report.starts_with("energy.csv\n"));
    }

    #[test]
    fn a_plain_report_carries_no_escape_codes() {
        let profile = profile_bytes(b"id,name,value\nA-17,Main,21.5\nA-18,North,20.9\n").unwrap();
        let report = render_profile("energy.csv", Some(64), &profile, Rendering::Plain);

        assert!(!report.contains(ESCAPE));
    }
}
