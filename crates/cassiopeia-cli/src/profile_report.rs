use cassiopeia_csv_dialect::{dialect::Dialect, statement::DialectStatement};
use cassiopeia_data_profiler::{
    metadata::{FormatMetadata, GribEdition},
    profile::Profile,
};
use cassiopeia_terminal_style::{
    byte_size::human_size,
    field::field,
    paint::{join, paint},
    palette::{ACCENT, MUTED, PRIMARY},
    rendering::Rendering,
};
use serde_json::Value;

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
        FormatMetadata::Csv(csv) => csv_line(csv.dialect(), rendering),
        FormatMetadata::Grib(grib) => Some(grib_line(grib.edition(), rendering)),
    }
}

/// Renders the CSV dialect line in the member names and value syntax of a manifest input's
/// `dialect`, so a member detection got wrong can be copied into a manifest and corrected there.
/// `None` only if the dialect cannot be written as a statement.
fn csv_line(dialect: &Dialect, rendering: Rendering) -> Option<String> {
    let Value::Object(members) = serde_json::to_value(DialectStatement::from(dialect)).ok()? else {
        return None;
    };
    let fields: Vec<String> = members.iter().map(|(name, value)| field(name, &value.to_string(), rendering)).collect();
    Some(join(&fields, rendering))
}

/// Renders the GRIB metadata line: the edition read from the Indicator Section.
fn grib_line(edition: GribEdition, rendering: Rendering) -> String {
    let number = match edition {
        GribEdition::V1 => "1",
        GribEdition::V2 => "2",
    };
    field("edition", number, rendering)
}

#[cfg(test)]
mod tests {
    use crate::profile_report::{csv_line, grib_line, metadata_line, render_profile};
    use cassiopeia_csv_dialect::{
        character_encoding::CharacterEncoding,
        decimal_separator::DecimalSeparator,
        dialect::Dialect,
        dialect_byte::DialectByte,
        header_rows::HeaderRows,
        line_terminator::LineTerminator,
        preamble::Preamble,
        quote::Quote,
        value_notation::ValueNotation,
    };
    use cassiopeia_data_profiler::{
        encoding_hint::EncodingHint,
        metadata::{CsvMetadata, FormatMetadata, GribEdition, GribMetadata},
        profile_bytes,
    };
    use cassiopeia_terminal_style::rendering::Rendering;
    use encoding_rs::{UTF_8, WINDOWS_1252};
    use pretty_assertions::assert_eq;

    const ESCAPE: char = '\u{1b}';

    fn comma_dialect() -> Dialect {
        Dialect {
            delimiter: DialectByte::COMMA,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::ONE,
            escape: None,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
        }
    }

    #[test]
    fn a_csv_line_states_the_dialect_in_manifest_member_names() {
        let line = csv_line(&comma_dialect(), Rendering::Plain).unwrap();

        assert_eq!(
            line,
            "delimiter \",\" \u{b7} quote \"\\\"\" \u{b7} skipRows 0 \u{b7} headerRows 1 \u{b7} terminator \"lf\" \u{b7} encoding \"UTF-8\""
        );
    }

    #[test]
    fn a_headerless_tab_csv_line_states_header_false_and_no_quoting() {
        let dialect = Dialect {
            delimiter: DialectByte::TAB,
            quote_char: Quote::Disabled,
            header_rows: HeaderRows::NONE,
            terminator: LineTerminator::Crlf,
            encoding: CharacterEncoding::new(WINDOWS_1252),
            ..comma_dialect()
        };
        let line = csv_line(&dialect, Rendering::Plain).unwrap();

        assert_eq!(
            line,
            "delimiter \"\\t\" \u{b7} quote \"none\" \u{b7} skipRows 0 \u{b7} header false \u{b7} terminator \"crlf\" \u{b7} encoding \"windows-1252\""
        );
    }

    #[test]
    fn a_csv_line_states_a_preamble_and_a_stacked_header() {
        let dialect = Dialect {
            delimiter: DialectByte::SEMICOLON,
            preamble: Preamble::new(1),
            header_rows: HeaderRows::new(3),
            ..comma_dialect()
        };
        let line = csv_line(&dialect, Rendering::Plain).unwrap();

        assert!(line.contains("skipRows 1 \u{b7} headerRows 3"), "{line}");
    }

    #[test]
    fn a_csv_line_states_a_value_notation_only_when_it_is_not_plain() {
        let dialect = Dialect {
            notation: ValueNotation {
                decimal_separator: DecimalSeparator::Comma,
                ..ValueNotation::PLAIN
            },
            ..comma_dialect()
        };

        assert!(csv_line(&dialect, Rendering::Plain).unwrap().ends_with("decimalSeparator \",\""));
        assert!(!csv_line(&comma_dialect(), Rendering::Plain).unwrap().contains("decimalSeparator"));
    }

    #[test]
    fn a_grib_line_states_the_edition() {
        assert_eq!(grib_line(GribEdition::V1, Rendering::Plain), "edition 1");
        assert_eq!(grib_line(GribEdition::V2, Rendering::Plain), "edition 2");
    }

    #[test]
    fn a_metadata_line_is_absent_for_a_format_without_metadata() {
        assert_eq!(metadata_line(None, Rendering::Plain), None);
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
        let profile = profile_bytes(b"id,name,value\nA-17,Main,21.5\nA-18,North,20.9\n", &EncodingHint::NONE).unwrap();
        let report = render_profile("energy.csv", Some(133_744_204), &profile, Rendering::Plain);
        let mut lines = report.lines();

        assert_eq!(lines.next().unwrap(), "energy.csv \u{b7} 127.5 MiB");
        let format = lines.next().unwrap();
        assert!(format.starts_with("CSV"));
        assert!(format.contains("% confidence"));
    }

    #[test]
    fn a_report_omits_the_size_when_it_is_unknown() {
        let profile = profile_bytes(b"id,name,value\nA-17,Main,21.5\nA-18,North,20.9\n", &EncodingHint::NONE).unwrap();
        let report = render_profile("energy.csv", None, &profile, Rendering::Plain);

        assert!(report.starts_with("energy.csv\n"));
    }

    #[test]
    fn a_plain_report_carries_no_escape_codes() {
        let profile = profile_bytes(b"id,name,value\nA-17,Main,21.5\nA-18,North,20.9\n", &EncodingHint::NONE).unwrap();
        let report = render_profile("energy.csv", Some(64), &profile, Rendering::Plain);

        assert!(!report.contains(ESCAPE));
    }
}
