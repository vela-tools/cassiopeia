use crate::{
    entity_store::{
        mapping_reads::MappingReads,
        write_batch::{WriteBatch, WriteBatches},
    },
    mapping_id::MappingId,
};
use cassiopeia_ir::source_position::SourcePosition;
use cassiopeia_mapping::source_reads::SourceReads;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Hands out a position later than every one handed out before it, so the records a test builds one
/// after another stand in the source in the order the test builds them.
pub(crate) fn next_position() -> SourcePosition {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    SourcePosition::first_of(0).advanced(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Hands out a batch later than every one handed out before it, so writes a test makes one after
/// another reach a store as successive resolver batches do.
pub(crate) fn next_batch() -> WriteBatch {
    static BATCHES: WriteBatches = WriteBatches::new();
    BATCHES.next()
}

/// The read sets of mappings that each read every key, whatever their id, so every disagreement a
/// store test merges counts.
pub(crate) struct ReadsEverything;

impl MappingReads for ReadsEverything {
    fn reads(&self, _mapping_id: MappingId) -> Option<&SourceReads> {
        static EVERYTHING: SourceReads = SourceReads::Everything;
        Some(&EVERYTHING)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_order::{next_batch, next_position};

    #[test]
    fn each_position_handed_out_comes_after_the_one_before_it() {
        let first = next_position();
        let second = next_position();
        let third = next_position();
        assert!(first < second);
        assert!(second < third);
    }

    #[test]
    fn each_batch_handed_out_comes_after_the_one_before_it() {
        let first = next_batch();
        assert!(first < next_batch());
    }
}
