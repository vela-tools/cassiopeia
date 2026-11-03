use cassiopeia_csv_dialect::line_terminator::LineTerminator;

/// Scan granularity for [`sample_of`].
pub const CHUNK: usize = 64 * 1024;

/// The fewest lines a sample must hold before header scoring can run: row 0 plus enough data rows
/// for both the header hypothesis (which needs two full rows) and per-column typing.
///
/// Lines stand in for records here because where a record ends depends on the quote, which is not
/// detected yet. A quoted field with a line break makes a record span several lines, so a sample of
/// very wide multi-line records can hold fewer records than this; it is then scored on fewer rows,
/// and is still cut at a record boundary.
const MIN_SAMPLE_LINES: usize = 5;

/// Hard ceiling on the dialect sample. It bounds the read, and the scoring work of the candidate
/// dialects, for a file whose rows are pathologically wide: a single row can exceed the soft
/// target on its own, so a fixed-size sample could hold no complete data row at all.
const MAX_SAMPLE_BYTES: usize = 4 * 1024 * 1024;

/// The soft target size of the dialect sample; see [`sample_of`].
pub const SAMPLE_SIZE: usize = 32 * 1024;

/// Whether the dialect sample is the whole content or a leading part cut from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleExtent {
    /// The sample is the whole content, so its last record is the file's last record.
    Whole,
    /// The sample was cut from the content, so its last record may be cut short.
    Cut,
}

/// The leading bytes of `bytes` the dialect is scored on, large enough to score a header and
/// bounded by a hard byte ceiling, and whether they are all of `bytes`.
///
/// A narrow CSV crosses [`MIN_SAMPLE_LINES`] line ends well inside the soft target `sample_size`,
/// so the sample stops at that target. A file whose rows are wider than the soft target keeps
/// sampling until it holds enough lines to tell a header from data, and [`MAX_SAMPLE_BYTES`]
/// caps a pathologically wide single row. Line ends are counted per chunk, never by rescanning
/// the sample, so the scan stays linear in the bytes sampled. The terminator is not detected
/// yet, so a chunk's line ends are its line feeds or its carriage returns, whichever are more:
/// a CRLF line holds one of each, and a file whose lines end with a lone CR has no line feed.
///
/// A cut sample ends wherever its last chunk does, even inside a record or a character; each
/// candidate dialect trims it to its complete records with
/// [`Dialect::complete_records`](cassiopeia_csv_dialect::dialect::Dialect::complete_records).
pub fn sample_of(bytes: &[u8], sample_size: usize) -> (&[u8], SampleExtent) {
    let mut end = 0usize;
    let mut line_ends = 0usize;
    for chunk in bytes.chunks(CHUNK) {
        line_ends += bytecount::count(chunk, b'\n').max(bytecount::count(chunk, b'\r'));
        end += chunk.len();
        if (end >= sample_size && line_ends >= MIN_SAMPLE_LINES) || end >= MAX_SAMPLE_BYTES {
            break;
        }
    }
    let extent = if end < bytes.len() { SampleExtent::Cut } else { SampleExtent::Whole };
    (&bytes[..end], extent)
}

/// The line terminator of `sample`: CRLF as soon as one line ends with it, a lone CR when the
/// sample has more carriage returns than line feeds, and LF otherwise.
///
/// The quote is not detected yet, so a line break inside a quoted field cannot be told from one
/// that ends a record; the more frequent byte is taken as the terminator, and the other as such a
/// quoted break. A CR-terminated file with a line feed in a quoted cell is still read by CR.
pub fn terminator_of(sample: &[u8]) -> LineTerminator {
    if sample.windows(2).any(|w| w == b"\r\n") {
        LineTerminator::Crlf
    } else if bytecount::count(sample, b'\r') > bytecount::count(sample, b'\n') {
        LineTerminator::Cr
    } else {
        LineTerminator::Lf
    }
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::sample::{CHUNK, MAX_SAMPLE_BYTES, SampleExtent, sample_of, terminator_of};
    use cassiopeia_csv_dialect::line_terminator::LineTerminator;
    use pretty_assertions::assert_eq;

    #[test]
    fn content_shorter_than_the_target_is_sampled_whole() {
        let bytes = b"id;name\n1;a\n2;b\n";

        assert_eq!(sample_of(bytes, 1024), (&bytes[..], SampleExtent::Whole));
    }

    #[test]
    fn content_with_enough_lines_past_the_target_is_cut_after_the_chunk_reaching_it() {
        let bytes = "1;a\n".repeat(CHUNK);

        let (sample, extent) = sample_of(bytes.as_bytes(), 16);

        assert_eq!(sample.len(), CHUNK);
        assert_eq!(extent, SampleExtent::Cut);
    }

    #[test]
    fn a_row_wider_than_the_target_keeps_the_sample_growing_until_enough_lines() {
        let mut bytes = "x".repeat(CHUNK * 2);
        bytes.push_str(&"\n1".repeat(5));
        bytes.push_str(&"\n2".repeat(CHUNK));

        let (sample, extent) = sample_of(bytes.as_bytes(), 16);

        assert_eq!(sample.len(), CHUNK * 3);
        assert_eq!(extent, SampleExtent::Cut);
    }

    #[test]
    fn a_single_pathologically_wide_row_is_capped_at_the_hard_ceiling() {
        let bytes = vec![b'x'; MAX_SAMPLE_BYTES + CHUNK];

        assert_eq!(sample_of(&bytes, 16).0.len(), MAX_SAMPLE_BYTES);
    }

    #[test]
    fn one_crlf_line_end_makes_the_terminator_crlf() {
        assert_eq!(terminator_of(b"a;b\r\n1;2\n"), LineTerminator::Crlf);
    }

    #[test]
    fn more_carriage_returns_than_line_feeds_make_the_terminator_a_lone_cr() {
        assert_eq!(terminator_of(b"a;b\r1;\"x\ny\"\r2;3\r"), LineTerminator::Cr);
    }

    #[test]
    fn line_feeds_alone_make_the_terminator_lf() {
        assert_eq!(terminator_of(b"a;b\n1;2\n"), LineTerminator::Lf);
        assert_eq!(terminator_of(b""), LineTerminator::Lf);
    }
}
