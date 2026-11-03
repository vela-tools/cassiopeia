/// The most bytes of evidence lines taken from the content as a whole. Detection settles on a few
/// hundred bytes of accented text; the cap bounds detection on a file whose every line carries one,
/// where weighing every line costs about a minute per GiB.
const MAX_EVIDENCE_BYTES: usize = 8 * 1024 * 1024;

/// The equal parts of the content the evidence budget is spread across, so the evidence of a large
/// file comes from all of it rather than from its start alone.
const SEGMENTS: usize = 16;

/// Whether `byte` ends a line: a line feed, or a carriage return, which alone ends every line of a
/// file written with classic Mac OS line endings. A CRLF line end splits into a line and the lone
/// `\n` after it, which is ASCII and never evidence.
const fn is_line_end(byte: u8) -> bool {
    byte == b'\n' || byte == b'\r'
}

/// The lines of `bytes` that carry evidence of its character encoding, for a detector to weigh.
///
/// A line of ASCII alone reads the same in every encoding a detector can guess, so only lines
/// holding a non-ASCII byte are taken, each whole, so every accented word keeps the letters around
/// it. The content is split into [`SEGMENTS`] line-aligned parts, and each gives at most its share
/// of [`MAX_EVIDENCE_BYTES`]; a line that crosses its part's share is still taken whole.
pub fn evidence_lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    line_aligned_segments(bytes, SEGMENTS).flat_map(|segment| {
        let mut taken = 0usize;
        segment
            .split_inclusive(|&byte| is_line_end(byte))
            .filter(|line| !line.is_ascii())
            .take_while(move |line| {
                let within_share = taken < MAX_EVIDENCE_BYTES / SEGMENTS;
                taken += line.len();
                within_share
            })
    })
}

/// Splits `bytes` into `count` consecutive parts of about equal length, each ending after a line end
/// or at the end of `bytes`, so no line is split between two parts. A part may be empty.
fn line_aligned_segments(bytes: &[u8], count: usize) -> impl Iterator<Item = &[u8]> {
    let mut start = 0usize;
    (1..=count).map(move |part| {
        let target = (bytes.len() / count * part).max(start);
        let end = if part == count {
            bytes.len()
        } else {
            bytes
                .get(target..)
                .and_then(|rest| rest.iter().position(|&byte| is_line_end(byte)))
                .map_or(bytes.len(), |newline| target + newline + 1)
        };
        let segment = bytes.get(start..end).unwrap_or_default();
        start = end;
        segment
    })
}

#[cfg(test)]
mod tests {
    use crate::encoding_evidence::{MAX_EVIDENCE_BYTES, SEGMENTS, evidence_lines, line_aligned_segments};
    use pretty_assertions::assert_eq;

    #[test]
    fn only_lines_holding_a_non_ascii_byte_are_evidence() {
        let lines: Vec<&[u8]> = evidence_lines(b"id;name\n1;M\xfcller\n2;Station\n3;Stra\xdfe").collect();

        assert_eq!(lines, vec![&b"1;M\xfcller\n"[..], &b"3;Stra\xdfe"[..]]);
    }

    #[test]
    fn a_file_of_lone_carriage_returns_gives_its_accented_lines_alone() {
        let lines: Vec<&[u8]> = evidence_lines(b"id;name\r1;M\xfcller\r2;Station\r3;Stra\xdfe\r").collect();

        assert_eq!(lines, vec![&b"1;M\xfcller\r"[..], &b"3;Stra\xdfe\r"[..]]);
    }

    #[test]
    fn evidence_of_a_large_file_of_lone_carriage_returns_is_capped() {
        let bytes = b"M\xfcller;Stra\xdfe\r".repeat(2 * MAX_EVIDENCE_BYTES / 14);
        let taken: usize = evidence_lines(&bytes).map(<[u8]>::len).sum();

        assert!(taken <= MAX_EVIDENCE_BYTES + SEGMENTS * 14);
    }

    #[test]
    fn segments_of_a_file_of_lone_carriage_returns_split_no_line() {
        let bytes = b"id;name\r1;alpha\r2;beta\r3;gamma\r4;delta\r";
        let segments: Vec<&[u8]> = line_aligned_segments(bytes, 4).collect();

        assert_eq!(segments.concat(), bytes.to_vec());
        assert!(segments.iter().all(|segment| segment.is_empty() || segment.ends_with(b"\r")));
    }

    #[test]
    fn segments_cover_every_byte_once_and_split_no_line() {
        let bytes = b"id;name\n1;alpha\n2;beta\n3;gamma\n4;delta\n";
        let segments: Vec<&[u8]> = line_aligned_segments(bytes, 4).collect();

        assert_eq!(segments.concat(), bytes.to_vec());
        assert!(segments.iter().all(|segment| segment.is_empty() || segment.ends_with(b"\n")));
    }

    #[test]
    fn content_shorter_than_the_segment_count_is_one_segment_and_empty_ones() {
        let segments: Vec<&[u8]> = line_aligned_segments(b"a\xfc", SEGMENTS).collect();

        assert_eq!(segments.concat(), b"a\xfc".to_vec());
        assert_eq!(segments.len(), SEGMENTS);
    }

    #[test]
    fn evidence_is_capped_and_still_reaches_the_end_of_a_large_file() {
        // Accented lines fill the first half far past the cap; the second half is ASCII but for its
        // last line, which a cap spent at the start of the file would never reach.
        let mut bytes = b"M\xfcller;Stra\xdfe\n".repeat(2 * MAX_EVIDENCE_BYTES / 14);
        bytes.extend_from_slice(&b"Station;Gasse\n".repeat(2 * MAX_EVIDENCE_BYTES / 14));
        bytes.extend_from_slice(b"last;\xe9t\xe9\n");
        let lines: Vec<&[u8]> = evidence_lines(&bytes).collect();
        let taken: usize = lines.iter().map(|line| line.len()).sum();

        assert!(taken <= MAX_EVIDENCE_BYTES + SEGMENTS * 14);
        assert_eq!(lines.last(), Some(&&b"last;\xe9t\xe9\n"[..]));
    }
}
