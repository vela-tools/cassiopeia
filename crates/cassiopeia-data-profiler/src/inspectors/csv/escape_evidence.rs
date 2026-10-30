use cassiopeia_csv_dialect::{dialect_byte::DialectByte, escape::Escape};

/// Where a quote-aware scan of CSV bytes stands, following the states `csv_core` reads a record in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    /// At the start of a field, where a quote opens a quoted field.
    FieldStart,
    /// Inside a field that did not start with a quote, where a quote is literal text.
    Unquoted,
    /// Inside a quoted field.
    Quoted,
    /// Right after the escape byte inside a quoted field, so the next byte is literal.
    Escaped,
    /// Right after the quote that closed a quoted field, and the byte in front of that quote.
    Closed(u8),
}

/// What a scan of a sample under one escape found: the closing quotes followed by text, which
/// `csv_core` appends to the field instead of rejecting the record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct StrayCloses {
    /// Every closing quote followed by a byte that neither ends the field nor doubles the quote.
    total: usize,
    /// Those of them with the suspected escape byte directly in front.
    after_escape: usize,
}

impl StrayCloses {
    /// Scans `sample`, split by `delimiter` and quoted by `quote`, under `escape`, counting the stray
    /// closing quotes and those with `suspect` in front.
    fn of(sample: &[u8], delimiter: DialectByte, quote: DialectByte, escape: Escape, suspect: DialectByte) -> StrayCloses {
        let (delimiter, quote, suspect) = (delimiter.byte(), quote.byte(), suspect.byte());
        let mut strays = StrayCloses::default();
        let mut state = ScanState::FieldStart;
        let mut previous = 0u8;
        for &byte in sample {
            let ends_field = byte == delimiter || byte == b'\n' || byte == b'\r';
            state = match state {
                ScanState::FieldStart | ScanState::Unquoted if ends_field => ScanState::FieldStart,
                // A quote right after a closing quote is a doubled quote, read as one under either
                // escape, since both readers leave `double_quote` on.
                ScanState::FieldStart | ScanState::Closed(_) if byte == quote => ScanState::Quoted,
                ScanState::FieldStart | ScanState::Unquoted => ScanState::Unquoted,
                ScanState::Quoted if byte == quote => ScanState::Closed(previous),
                ScanState::Quoted if escape.byte() == Some(byte) => ScanState::Escaped,
                ScanState::Quoted | ScanState::Escaped => ScanState::Quoted,
                ScanState::Closed(_) if ends_field => ScanState::FieldStart,
                ScanState::Closed(before) => {
                    strays.total += 1;
                    if before == suspect {
                        strays.after_escape += 1;
                    }
                    ScanState::Unquoted
                }
            };
            previous = byte;
        }
        strays
    }
}

/// Whether `sample` reads as quoting with `quote` and escaping it with `escape` rather than by
/// doubling it (RFC 4180 clause 2.7).
///
/// Both read most such files without a parse error, because `csv_core` appends the text after a
/// closing quote to the field instead of rejecting it: under doubling, `"say \"hi\""` closes at the
/// first `\"` and reads as `say \hi\""`. That stray text after a closing quote is the evidence, so
/// the escape is backed only when doubling leaves at least one closing quote with `escape` in front
/// followed by text, and the escape leaves strictly fewer stray closing quotes than doubling does.
///
/// An RFC 4180 file never backs it: a doubled quote `""` is no stray close under doubling, and a
/// quoted Windows path ending in a backslash, `"C:\data\",1`, closes before a delimiter, so doubling
/// finds no stray close in front of which the escape stands.
pub(crate) fn backs_escape(sample: &[u8], delimiter: DialectByte, quote: DialectByte, escape: DialectByte) -> bool {
    let suspected = [escape.byte(), quote.byte()];
    if !sample.windows(2).any(|pair| pair == suspected) {
        return false;
    }
    let doubled = StrayCloses::of(sample, delimiter, quote, Escape::Doubled, escape);
    doubled.after_escape > 0 && StrayCloses::of(sample, delimiter, quote, Escape::Char(escape), escape).total < doubled.total
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::escape_evidence::{StrayCloses, backs_escape};
    use cassiopeia_csv_dialect::{dialect_byte::DialectByte, escape::Escape};
    use pretty_assertions::assert_eq;

    fn backs_backslash(sample: &str) -> bool {
        backs_escape(sample.as_bytes(), DialectByte::COMMA, DialectByte::DOUBLE_QUOTE, DialectByte::BACKSLASH)
    }

    #[test]
    fn backslash_escaped_quotes_back_the_backslash() {
        assert!(backs_backslash("id,name,value\n1,\"say \\\"hi\\\"\",10\n2,plain,20\n3,\"a\\\"b\",30\n"));
    }

    #[test]
    fn doubled_quotes_do_not_back_the_backslash() {
        assert!(!backs_backslash("id,name\n1,\"say \"\"hi\"\"\"\n2,plain\n"));
    }

    #[test]
    fn a_quoted_windows_path_ending_in_a_backslash_does_not_back_the_backslash() {
        assert!(!backs_backslash("path,count\n\"C:\\data\\\",1\n\"D:\\x\\\",2\n"));
    }

    #[test]
    fn a_file_without_a_backslash_before_a_quote_does_not_back_the_backslash() {
        assert!(!backs_backslash("id,path\n1,C:\\data\n2,\"plain\"\n"));
    }

    #[test]
    fn a_quote_inside_an_unquoted_field_is_no_stray_close() {
        let strays = StrayCloses::of(
            b"id,size\n1,12\\\"\n",
            DialectByte::COMMA,
            DialectByte::DOUBLE_QUOTE,
            Escape::Doubled,
            DialectByte::BACKSLASH,
        );

        assert_eq!(strays, StrayCloses::default());
    }

    #[test]
    fn text_after_a_closing_quote_is_a_stray_close() {
        let strays = StrayCloses::of(
            b"1,\"a\\\"b\"\n",
            DialectByte::COMMA,
            DialectByte::DOUBLE_QUOTE,
            Escape::Doubled,
            DialectByte::BACKSLASH,
        );

        assert_eq!(strays, StrayCloses { total: 1, after_escape: 1 });
    }
}
