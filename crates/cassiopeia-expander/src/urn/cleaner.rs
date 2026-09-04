use deunicode::deunicode;
use unicode_normalization::UnicodeNormalization;

/// Builds the byte-classification table: `true` for every byte in `[A-Za-z0-9_-]`.
const fn build_allowed_table() -> [bool; 256] {
    let mut table = [false; 256];
    let mut byte: u8 = 0;
    loop {
        let is_digit = byte >= b'0' && byte <= b'9';
        let is_upper = byte >= b'A' && byte <= b'Z';
        let is_lower = byte >= b'a' && byte <= b'z';
        if is_digit || is_upper || is_lower || byte == b'-' || byte == b'_' {
            table[byte as usize] = true;
        }
        if byte == u8::MAX {
            break;
        }
        byte += 1;
    }
    table
}

/// Whether a byte is allowed in a cleaned identifier, looked up in a compile-time table.
const ALLOWED_BYTE: [bool; 256] = build_allowed_table();

/// Normalizes arbitrary text into a URN-safe identifier segment.
pub(crate) struct Cleaner;

impl Cleaner {
    /// Cleans an identifier: keeps only `[A-Za-z0-9_-]`, transliterating non-ASCII first.
    ///
    /// Pure-ASCII input takes a fast path that skips transliteration and Unicode normalization
    /// entirely, and returns the input untouched when it is already clean. Non-ASCII input is run
    /// through `deunicode` and NFC-normalized before the same character filter is applied.
    pub(crate) fn clean(source: &str) -> String {
        if source.is_ascii() {
            if source.bytes().all(|byte| ALLOWED_BYTE[byte as usize]) {
                return source.to_owned();
            }
            return source.bytes().filter(|&byte| ALLOWED_BYTE[byte as usize]).map(char::from).collect();
        }

        deunicode(source)
            .nfc()
            .filter(|character| character.is_alphanumeric() || *character == '-' || *character == '_')
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::urn::cleaner::Cleaner;

    #[test]
    fn already_clean_ascii_is_returned_unchanged() {
        assert_eq!(Cleaner::clean("hello-world_42"), "hello-world_42");
    }

    #[test]
    fn a_disallowed_ascii_byte_is_dropped() {
        assert_eq!(Cleaner::clean("hello world"), "helloworld");
    }

    #[test]
    fn several_disallowed_ascii_bytes_are_dropped() {
        assert_eq!(Cleaner::clean("  a!b@c  "), "abc");
    }

    #[test]
    fn non_ascii_is_transliterated() {
        assert_eq!(Cleaner::clean("München"), "Munchen");
    }

    #[test]
    fn empty_input_stays_empty() {
        assert_eq!(Cleaner::clean(""), "");
    }

    #[test]
    fn a_leading_run_of_symbols_and_underscores_is_preserved() {
        assert_eq!(Cleaner::clean("---___aaa"), "---___aaa");
    }

    #[test]
    fn a_long_clean_run_survives_intact() {
        let input = "abcdefghijklmnopqrstuvwxyz0123456789_-";
        assert_eq!(Cleaner::clean(input), input);
    }

    #[test]
    fn a_disallowed_byte_in_the_middle_is_removed() {
        assert_eq!(Cleaner::clean("abcdefg!hijklmno"), "abcdefghijklmno");
    }
}
