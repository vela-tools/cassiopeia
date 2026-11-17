use crate::entity_store::{
    error::{EntityStoreError, Result},
    stored_slot::StoredSlot,
};
use cassiopeia_ir::source_position::{SOURCE_POSITION_BYTES, SourcePosition};
use rmp_serde::encode::write;

/// One mapping's slot for an id as the disk-backed current-state store keeps it: the position of the
/// first record the mapping resolved to the id, as its big-endian bytes, then the slot in
/// `MessagePack`.
///
/// The position leads the value so a batch reaching an id learns where the id's first record stands
/// from every slot of the id while decoding only the slots its own records land on. This is an
/// internal store encoding and is unrelated to any ingested data format.
pub(crate) fn encode_slot(first: SourcePosition, slot: &StoredSlot) -> Result<Vec<u8>> {
    let mut value = Vec::from(first.to_be_bytes());
    write(&mut value, slot)?;
    Ok(value)
}

/// The position of the first record of the slot [`encode_slot`] wrote as `value`.
pub(crate) fn slot_first(value: &[u8]) -> Result<SourcePosition> {
    split(value).map(|(first, _slot)| first)
}

/// The slot [`encode_slot`] wrote as `value`, with the position of its first record.
pub(crate) fn decode_slot(value: &[u8]) -> Result<(SourcePosition, StoredSlot)> {
    let (first, slot) = split(value)?;
    Ok((first, rmp_serde::from_slice(slot)?))
}

/// The position `value` leads with, and the encoded slot after it.
fn split(value: &[u8]) -> Result<(SourcePosition, &[u8])> {
    let (first, slot) = value
        .split_first_chunk::<SOURCE_POSITION_BYTES>()
        .ok_or(EntityStoreError::TruncatedPosition { length: value.len() })?;
    Ok((SourcePosition::from_be_bytes(*first), slot))
}

#[cfg(test)]
mod tests {
    use crate::entity_store::{
        error::EntityStoreError,
        mapping_expiry::MappingExpiry,
        redb_slot_value::{decode_slot, encode_slot, slot_first},
        stored_slot::StoredSlot,
    };
    use cassiopeia_ir::source_position::SourcePosition;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::num::NonZeroU64;

    fn slot() -> StoredSlot {
        StoredSlot::Merged {
            records: NonZeroU64::MIN.saturating_add(2),
            data: json!({"name": "Main"}),
            conflict: None,
            expires_at: MappingExpiry::first(None),
        }
    }

    #[test]
    fn a_slot_reads_back_with_the_position_of_its_first_record() {
        let first = SourcePosition::first_of(2).advanced(41).nth_fragment(1);
        let value = encode_slot(first, &slot()).unwrap();

        assert_eq!(slot_first(&value).unwrap(), first);
        assert_eq!(decode_slot(&value).unwrap(), (first, slot()));
    }

    #[test]
    fn a_value_too_short_to_hold_a_position_is_refused() {
        let value = [0_u8; 15];

        assert!(matches!(slot_first(&value), Err(EntityStoreError::TruncatedPosition { length: 15 })));
        assert!(matches!(decode_slot(&value), Err(EntityStoreError::TruncatedPosition { length: 15 })));
    }
}
