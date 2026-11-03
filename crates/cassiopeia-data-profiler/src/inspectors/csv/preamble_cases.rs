//! Tables below a preamble whose records span several lines, inspected and then read back under the
//! detected dialect.
//!
//! The preamble is counted in CSV records, never physical lines (RFC 4180 clause 2.6 lets a quoted
//! field hold a line break), so each case checks both that the inspector counts the records right
//! and that reading the file under its dialect yields the header and every data row intact.

use crate::{content::Content, inspectors::csv::CsvInspector};
use cassiopeia_csv_dialect::{
    dialect_byte::DialectByte,
    header_rows::HeaderRows,
    line_terminator::LineTerminator,
    preamble::Preamble,
    statement::DialectStatement,
};
use encoding_rs::{UTF_8, UTF_16LE};
use pretty_assertions::assert_eq;

/// The bytes of `text` in UTF-16LE, behind its byte order mark.
fn utf_16le_with_bom(text: &str) -> Vec<u8> {
    [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect()
}

/// Expands each case into a test that inspects `input` with nothing stated, checks the detected
/// delimiter, terminator, encoding, preamble, and single header row, and then reads `input`, decoded
/// from the detected encoding, under the detected dialect, checking its header and every data row.
///
/// `input` is anything that views as bytes, so a string literal and a built `Vec<u8>` both fit.
macro_rules! preamble_cases {
    ($(
        $name:ident {
            input: $input:expr,
            delimiter: $delimiter:expr,
            terminator: $terminator:expr,
            encoding: $encoding:expr,
            preamble: $preamble:expr,
            headers: [$($header:expr),* $(,)?],
            rows: [$([$($cell:expr),* $(,)?]),* $(,)?] $(,)?
        }
    )*) => {
        $(
            #[test]
            fn $name() {
                let input = $input;
                let bytes: &[u8] = input.as_ref();
                let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(bytes)).unwrap();
                let decoded = dialect.character_encoding().decode(bytes).0;
                let mut reader = dialect.reader(decoded.as_bytes()).unwrap();
                let rows: Vec<_> = reader.records().map(Result::unwrap).collect();

                assert_eq!(dialect.delimiter, $delimiter);
                assert_eq!(dialect.terminator, $terminator);
                assert_eq!(dialect.character_encoding(), $encoding);
                assert_eq!(dialect.preamble, Preamble::new($preamble));
                assert_eq!(dialect.header_rows, HeaderRows::ONE);
                assert_eq!(reader.headers().unwrap(), vec![$($header),*]);
                assert_eq!(rows, vec![$(vec![$($cell),*]),*]);
            }
        )*
    };
}

preamble_cases! {
    a_quoted_two_line_title_in_a_crlf_file_is_one_preamble_record {
        input: "\"Lumes report\r\ngenerated 25.10.26\";v2.10\r\nStation;NO2;O3\r\nSTEF;12,45;54,55\r\nAKA;9,80;40,10\r\n",
        delimiter: DialectByte::SEMICOLON,
        terminator: LineTerminator::Crlf,
        encoding: UTF_8,
        preamble: 1,
        headers: ["Station", "NO2", "O3"],
        rows: [["STEF", "12,45", "54,55"], ["AKA", "9,80", "40,10"]],
    }

    a_quoted_two_line_title_in_a_comma_delimited_file_is_one_preamble_record {
        input: "\"Lumes report\ngenerated 25.10.26\",v2.10\nStation,NO2,O3\nSTEF,12,54\nAKA,9,40\n",
        delimiter: DialectByte::COMMA,
        terminator: LineTerminator::Lf,
        encoding: UTF_8,
        preamble: 1,
        headers: ["Station", "NO2", "O3"],
        rows: [["STEF", "12", "54"], ["AKA", "9", "40"]],
    }

    a_quoted_two_line_title_in_a_utf_16le_file_with_a_byte_order_mark_is_one_preamble_record {
        input: utf_16le_with_bom("\"Lumes report\ngenerated 25.10.26\";v2.10\nStation;NO2;O3\nSTEF;12,45;54,55\nAKA;9,80;40,10\n"),
        delimiter: DialectByte::SEMICOLON,
        terminator: LineTerminator::Lf,
        encoding: UTF_16LE,
        preamble: 1,
        headers: ["Station", "NO2", "O3"],
        rows: [["STEF", "12,45", "54,55"], ["AKA", "9,80", "40,10"]],
    }

    a_delimiter_inside_a_quoted_two_line_title_does_not_widen_it_to_the_table {
        input: "\"Lumes;report\nv2;x\";y\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\n",
        delimiter: DialectByte::SEMICOLON,
        terminator: LineTerminator::Lf,
        encoding: UTF_8,
        preamble: 1,
        headers: ["Station", "NO2", "O3"],
        rows: [["STEF", "12", "54"], ["AKA", "9", "40"]],
    }

    a_one_field_title_spanning_three_lines_is_one_preamble_record {
        input: "\"Lumes report\ngenerated\n25.10.26\"\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\n",
        delimiter: DialectByte::SEMICOLON,
        terminator: LineTerminator::Lf,
        encoding: UTF_8,
        preamble: 1,
        headers: ["Station", "NO2", "O3"],
        rows: [["STEF", "12", "54"], ["AKA", "9", "40"]],
    }

    two_quoted_multi_line_titles_are_two_preamble_records {
        input: "\"T1\nx\";a\n\"T2\ny\";b;c;d\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\n",
        delimiter: DialectByte::SEMICOLON,
        terminator: LineTerminator::Lf,
        encoding: UTF_8,
        preamble: 2,
        headers: ["Station", "NO2", "O3"],
        rows: [["STEF", "12", "54"], ["AKA", "9", "40"]],
    }

    a_quoted_two_line_title_padded_to_an_all_text_table_is_one_preamble_record {
        input: "\"Air quality\nreport\";;\nStation;Name;Kind\nSTEF;Stefan;urban\nAKA;Akh;rural\n",
        delimiter: DialectByte::SEMICOLON,
        terminator: LineTerminator::Lf,
        encoding: UTF_8,
        preamble: 1,
        headers: ["Station", "Name", "Kind"],
        rows: [["STEF", "Stefan", "urban"], ["AKA", "Akh", "rural"]],
    }

    a_dense_quoted_two_line_title_as_wide_as_a_numeric_table_is_one_preamble_record {
        input: "\"Lumes report\ngenerated 25.10.26\";v2.10;x\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\n",
        delimiter: DialectByte::SEMICOLON,
        terminator: LineTerminator::Lf,
        encoding: UTF_8,
        preamble: 1,
        headers: ["Station", "NO2", "O3"],
        rows: [["STEF", "12", "54"], ["AKA", "9", "40"]],
    }

    a_quoted_line_break_in_the_first_data_row_below_a_title_stays_in_its_cell {
        input: "Lumes;v2.10\nStation;Note;O3\nSTEF;\"line1\nline2\";54\nAKA;plain;40\n",
        delimiter: DialectByte::SEMICOLON,
        terminator: LineTerminator::Lf,
        encoding: UTF_8,
        preamble: 1,
        headers: ["Station", "Note", "O3"],
        rows: [["STEF", "line1\nline2", "54"], ["AKA", "plain", "40"]],
    }

    a_title_opening_a_quote_it_never_closes_is_one_preamble_record {
        input: "\"Lumes v2.10 live\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\nTAB;7;38\n",
        delimiter: DialectByte::SEMICOLON,
        terminator: LineTerminator::Lf,
        encoding: UTF_8,
        preamble: 1,
        headers: ["Station", "NO2", "O3"],
        rows: [["STEF", "12", "54"], ["AKA", "9", "40"], ["TAB", "7", "38"]],
    }

    a_blank_line_below_a_quoted_two_line_title_is_no_preamble_record {
        input: "\"Lumes report\ngenerated 25.10.26\";v2.10\n\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\n",
        delimiter: DialectByte::SEMICOLON,
        terminator: LineTerminator::Lf,
        encoding: UTF_8,
        preamble: 1,
        headers: ["Station", "NO2", "O3"],
        rows: [["STEF", "12", "54"], ["AKA", "9", "40"]],
    }
}
