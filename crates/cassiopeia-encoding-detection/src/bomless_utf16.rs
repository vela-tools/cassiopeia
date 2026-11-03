use encoding_rs::{Encoding, UTF_16BE, UTF_16LE};

/// The most leading bytes weighed: enough code units to see a file's delimiters and line ends,
/// bounded so the check stays constant-time however large the file.
const WINDOW: usize = 8 * 1024;

/// The fewest code units in the window, as a fraction `1 / N`, that must hold a NUL in one half for
/// the content to read as UTF-16. Every delimiter, quote, digit, and line end of a CSV is a code unit
/// below U+0100, whose high byte is NUL, so even a table of Cyrillic or CJK names clears it.
const MIN_NUL_SHARE: usize = 8;

/// How many times more NULs the one half of the code units must hold than the other.
const MIN_NUL_RATIO: usize = 10;

/// The UTF-16 byte order of `bytes` when they carry no byte order mark: the encoding whose code
/// units put NUL in the high byte of a sizable share of them, and almost never in the low byte.
///
/// The WHATWG Encoding Standard recognises UTF-16 by its BOM alone, and `chardetng` never guesses
/// it, since web pages are not sent as UTF-16 without one. Files are: a spreadsheet's "Unicode
/// text" export is UTF-16LE, often with no BOM. A NUL byte is no character of text in any encoding `chardetng`
/// guesses, so a file with NULs in every other byte, and an even byte count, is UTF-16 in the byte
/// order those NULs mark. Binary content holds NULs at random in both halves and is not taken.
pub fn bomless_utf16(bytes: &[u8]) -> Option<&'static Encoding> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let (units, _) = bytes[..bytes.len().min(WINDOW)].as_chunks::<2>();
    let (first_nuls, second_nuls) = units.iter().fold((0usize, 0usize), |(first, second), [low, high]| {
        (first + usize::from(*low == 0), second + usize::from(*high == 0))
    });
    let marks = |nuls: usize, other: usize| nuls > 0 && nuls * MIN_NUL_SHARE >= units.len() && other * MIN_NUL_RATIO <= nuls;
    if marks(second_nuls, first_nuls) {
        Some(UTF_16LE)
    } else if marks(first_nuls, second_nuls) {
        Some(UTF_16BE)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::bomless_utf16::bomless_utf16;
    use encoding_rs::{UTF_16BE, UTF_16LE};
    use pretty_assertions::assert_eq;

    fn utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    fn utf16be(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_be_bytes).collect()
    }

    #[test]
    fn ascii_text_in_utf16le_reads_as_utf16le() {
        assert_eq!(bomless_utf16(&utf16le("id,name\n1,Ada\n2,Grace\n")), Some(UTF_16LE));
    }

    #[test]
    fn ascii_text_in_utf16be_reads_as_utf16be() {
        assert_eq!(bomless_utf16(&utf16be("id,name\n1,Ada\n2,Grace\n")), Some(UTF_16BE));
    }

    #[test]
    fn a_table_of_cyrillic_names_in_utf16le_reads_as_utf16le() {
        assert_eq!(bomless_utf16(&utf16le("ид;имя\n1;Иван Петров\n2;Мария Иванова\n")), Some(UTF_16LE));
    }

    #[test]
    fn utf8_text_has_no_utf16_byte_order() {
        assert_eq!(bomless_utf16("id,name\n1,Čapek\n2,Grace\n".as_bytes()), None);
    }

    #[test]
    fn an_odd_byte_count_is_no_utf16() {
        let mut bytes = utf16le("id,name\n1,Ada\n");
        bytes.push(b'\n');

        assert_eq!(bomless_utf16(&bytes), None);
    }

    #[test]
    fn nuls_in_both_halves_are_no_utf16() {
        assert_eq!(bomless_utf16(&[0, 0, 7, 0, 0, 9, 0, 0, 3, 0, 0, 4]), None);
    }

    #[test]
    fn empty_content_has_no_utf16_byte_order() {
        assert_eq!(bomless_utf16(&[]), None);
    }
}
