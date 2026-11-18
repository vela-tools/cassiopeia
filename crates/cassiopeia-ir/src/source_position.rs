use serde::{Deserialize, Serialize};

/// The number of bytes [`SourcePosition::to_be_bytes`] writes.
pub const SOURCE_POSITION_BYTES: usize = 16;

/// Where a fragment stands in the run's sources: the input that read its record, by its place in
/// the manifest, then the record's place among the records that input read, then the fragment's
/// place among the fragments the record expanded into.
///
/// Positions order fragments across inputs as the manifest lists them, within one input in the
/// order the input read their records, and within one record in the order it expanded into them, so
/// no two fragments of a run share one. The stages between reading and writing run in parallel and
/// hand records on in whatever order their threads finish, so every outcome that depends on the
/// order of records is decided by their positions instead: which record keeps a value, and in which
/// order the entities of a file are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourcePosition {
    /// The place of the record's input in the manifest.
    input: u32,
    /// The place of the record among the records its input read.
    record: u64,
    /// The place of the fragment among the fragments its record expanded into.
    fragment: u32,
}

impl SourcePosition {
    /// The position of the first fragment of the first record the input at `input` in the manifest
    /// reads.
    #[must_use]
    pub const fn first_of(input: u32) -> SourcePosition {
        SourcePosition { input, record: 0, fragment: 0 }
    }

    /// The position of the first fragment of the record `offset` records after this one's, in the
    /// same input.
    #[must_use]
    pub fn advanced(self, offset: usize) -> SourcePosition {
        SourcePosition {
            input: self.input,
            record: self.record.saturating_add(u64::try_from(offset).unwrap_or(u64::MAX)),
            fragment: 0,
        }
    }

    /// The position of the fragment at `index` among the fragments this one's record expanded into.
    #[must_use]
    pub fn nth_fragment(self, index: usize) -> SourcePosition {
        SourcePosition {
            fragment: u32::try_from(index).unwrap_or(u32::MAX),
            ..self
        }
    }

    /// The place in the manifest of the input that read the fragment's record.
    #[must_use]
    pub const fn input(self) -> u32 {
        self.input
    }

    /// The position as big-endian bytes, which sort bytewise exactly as the positions do, so a store
    /// can key by them and read them back in source order. [`from_be_bytes`](SourcePosition::from_be_bytes)
    /// reads them back.
    #[must_use]
    pub const fn to_be_bytes(self) -> [u8; SOURCE_POSITION_BYTES] {
        let [i0, i1, i2, i3] = self.input.to_be_bytes();
        let [r0, r1, r2, r3, r4, r5, r6, r7] = self.record.to_be_bytes();
        let [f0, f1, f2, f3] = self.fragment.to_be_bytes();
        [i0, i1, i2, i3, r0, r1, r2, r3, r4, r5, r6, r7, f0, f1, f2, f3]
    }

    /// The position [`to_be_bytes`](SourcePosition::to_be_bytes) wrote as `bytes`.
    #[must_use]
    pub const fn from_be_bytes(bytes: [u8; SOURCE_POSITION_BYTES]) -> SourcePosition {
        let [i0, i1, i2, i3, r0, r1, r2, r3, r4, r5, r6, r7, f0, f1, f2, f3] = bytes;
        SourcePosition {
            input: u32::from_be_bytes([i0, i1, i2, i3]),
            record: u64::from_be_bytes([r0, r1, r2, r3, r4, r5, r6, r7]),
            fragment: u32::from_be_bytes([f0, f1, f2, f3]),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::source_position::SourcePosition;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_input_s_records_follow_one_another_in_the_order_it_read_them() {
        let first = SourcePosition::first_of(0);
        assert!(first < first.advanced(1));
        assert!(first.advanced(1) < first.advanced(2));
        assert_eq!(first.advanced(2).advanced(3), first.advanced(5));
    }

    #[test]
    fn a_record_s_fragments_follow_one_another_and_come_before_the_next_record() {
        let record = SourcePosition::first_of(0).advanced(4);
        assert_eq!(record.nth_fragment(0), record);
        assert!(record.nth_fragment(0) < record.nth_fragment(1));
        assert!(record.nth_fragment(1_000) < record.advanced(1));
        assert_eq!(record.nth_fragment(3).advanced(1), record.advanced(1));
    }

    #[test]
    fn every_record_of_an_earlier_input_comes_before_any_record_of_a_later_one() {
        assert!(SourcePosition::first_of(0).advanced(1_000_000).nth_fragment(9) < SourcePosition::first_of(1));
        assert!(SourcePosition::first_of(1).advanced(3) < SourcePosition::first_of(2));
    }

    #[test]
    fn a_position_names_the_place_of_its_input_in_the_manifest_whatever_record_and_fragment_it_holds() {
        assert_eq!(SourcePosition::first_of(0).input(), 0);
        assert_eq!(SourcePosition::first_of(3).advanced(41).nth_fragment(2).input(), 3);
    }

    #[test]
    fn the_big_endian_bytes_sort_exactly_as_the_positions_do() {
        let positions = [
            SourcePosition::first_of(0),
            SourcePosition::first_of(0).nth_fragment(1),
            SourcePosition::first_of(0).nth_fragment(256),
            SourcePosition::first_of(0).advanced(1),
            SourcePosition::first_of(0).advanced(255),
            SourcePosition::first_of(0).advanced(256),
            SourcePosition::first_of(0).advanced(70_000),
            SourcePosition::first_of(1),
            SourcePosition::first_of(1).advanced(1),
            SourcePosition::first_of(300),
        ];
        for pair in positions.windows(2) {
            assert!(pair[0] < pair[1]);
            assert!(pair[0].to_be_bytes() < pair[1].to_be_bytes());
        }
    }

    #[test]
    fn the_big_endian_bytes_hold_the_input_then_the_record_then_the_fragment() {
        assert_eq!(
            SourcePosition::first_of(2).advanced(258).nth_fragment(3).to_be_bytes(),
            [0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 1, 2, 0, 0, 0, 3]
        );
    }

    #[test]
    fn a_position_reads_back_from_its_big_endian_bytes() {
        for position in [
            SourcePosition::first_of(0),
            SourcePosition::first_of(7).advanced(1_234_567_890_123).nth_fragment(5),
            SourcePosition::first_of(u32::MAX).advanced(usize::MAX).nth_fragment(usize::MAX),
        ] {
            assert_eq!(SourcePosition::from_be_bytes(position.to_be_bytes()), position);
        }
    }

    #[test]
    fn a_position_survives_a_serde_round_trip() {
        let position = SourcePosition::first_of(3).advanced(41).nth_fragment(2);
        let encoded = serde_json::to_string(&position).unwrap();
        assert_eq!(serde_json::from_str::<SourcePosition>(&encoded).unwrap(), position);
    }
}
