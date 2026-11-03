use encoding_rs::{Encoding, UTF_16BE, UTF_16LE};
use std::cmp::Ordering;

/// The most leading bytes weighed: enough code units to see a file's delimiters and line ends,
/// bounded so the check stays constant-time however large the file.
const WINDOW: usize = 8 * 1024;

/// What the code units below U+0100 show when a window is read in one byte order.
///
/// A unit below U+0100 has a NUL high byte. Text holds such units only as printable Latin-1
/// characters, tabs, and line ends. Binary content holds NUL units and control characters too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Reading {
    /// Units that are a printable Latin-1 character, a tab, or a line end.
    latin: usize,
    /// Units that are a line end, LF or CR.
    line_ends: usize,
    /// Units that no text holds: NUL and every control character but tab, LF, and CR.
    controls: usize,
}

impl Reading {
    /// Reads every unit of `units` with `unit`, which puts its two bytes in one byte order.
    fn of(units: &[[u8; 2]], unit: fn([u8; 2]) -> u16) -> Reading {
        units.iter().map(|&bytes| unit(bytes)).fold(Reading::default(), Reading::with)
    }

    /// This reading with `unit` added to it.
    fn with(self, unit: u16) -> Reading {
        let Ok(byte) = u8::try_from(unit) else {
            return self;
        };
        match byte {
            b'\n' | b'\r' => Reading {
                latin: self.latin + 1,
                line_ends: self.line_ends + 1,
                ..self
            },
            b'\t' => Reading { latin: self.latin + 1, ..self },
            control if char::from(control).is_control() => Reading {
                controls: self.controls + 1,
                ..self
            },
            _printable => Reading { latin: self.latin + 1, ..self },
        }
    }

    /// Whether the units read as text: some of them below U+0100, and none of those a control.
    const fn is_text(self) -> bool {
        self.controls == 0 && self.latin > 0
    }
}

/// The UTF-16 byte order of `bytes` when they carry no byte order mark: the order in which the
/// code units below U+0100 are all text, and which, when both are, reads more line ends.
///
/// The WHATWG Encoding Standard recognises UTF-16 by its BOM alone, and `chardetng` never guesses
/// it, since web pages are not sent as UTF-16 without one. Files are: a spreadsheet's "Unicode
/// text" export is UTF-16LE, often with no BOM. Every delimiter, quote, and line end of a CSV is a
/// unit below U+0100, so the right order reads them as text however few they are among the cells,
/// and a table of long CJK cells is recognised as surely as one of ASCII. Read in the wrong order,
/// a CJK unit can fall below U+0100 too, but then as a Latin-1 letter or a C1 control, never as the
/// line end each record ends with. Binary content holds NUL units and controls, and is not taken.
pub fn bomless_utf16(bytes: &[u8]) -> Option<&'static Encoding> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let (units, _) = bytes[..bytes.len().min(WINDOW)].as_chunks::<2>();
    let little = Reading::of(units, u16::from_le_bytes);
    let big = Reading::of(units, u16::from_be_bytes);
    match (little.is_text(), big.is_text()) {
        (true, false) => Some(UTF_16LE),
        (false, true) => Some(UTF_16BE),
        (false, false) => None,
        (true, true) => match (little.line_ends, little.latin).cmp(&(big.line_ends, big.latin)) {
            Ordering::Greater => Some(UTF_16LE),
            Ordering::Less => Some(UTF_16BE),
            Ordering::Equal => None,
        },
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

    fn cjk_table(rows: usize) -> String {
        let cell: String = "一二三四五六七八九十丁七万丈三上下不与丐丑专且丕世丘丙业丛东丝"
            .chars()
            .cycle()
            .take(300)
            .collect();
        (1..=rows).fold("编号,名称\n".to_owned(), |table, row| format!("{table}{row},{cell}\n"))
    }

    #[test]
    fn a_table_of_long_cjk_cells_in_utf16le_reads_as_utf16le() {
        let bytes = utf16le(&cjk_table(20));
        let ascii = bytes.chunks(2).filter(|unit| unit[1] == 0).count();
        assert!(ascii * 8 < bytes.len() / 2, "the table must be mostly CJK");

        assert_eq!(bomless_utf16(&bytes), Some(UTF_16LE));
    }

    #[test]
    fn a_table_of_long_cjk_cells_in_utf16be_reads_as_utf16be() {
        assert_eq!(bomless_utf16(&utf16be(&cjk_table(20))), Some(UTF_16BE));
    }

    #[test]
    fn a_single_line_without_a_line_end_reads_in_its_byte_order() {
        assert_eq!(bomless_utf16(&utf16le("id,name")), Some(UTF_16LE));
        assert_eq!(bomless_utf16(&utf16be("id,name")), Some(UTF_16BE));
    }

    #[test]
    fn a_control_character_below_u_0100_is_no_utf16_text() {
        assert_eq!(bomless_utf16(&utf16le("id,name\n1,\u{1}Ada\n")), None);
    }

    #[test]
    fn empty_content_has_no_utf16_byte_order() {
        assert_eq!(bomless_utf16(&[]), None);
    }
}
