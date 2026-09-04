use crate::dropped_geometry::DroppedGeometry;
use cassiopeia_geometry::error::GeometryError;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use indexmap::IndexMap;
use parking_lot::Mutex;

/// The refused geometry conversions gathered while one batch of entities is extracted.
///
/// The sink is shared, not owned per entity: a batch extracts across a Rayon pool, so every worker
/// records into the same one behind a shared reference.
///
/// The map is an [`IndexMap`] behind a [`Mutex`] rather than a `DashMap`: the messages a run prints
/// have to come out in the same order every time it is run over the same source, which a sharded map
/// cannot promise, and a sink is built per batch and stays empty in almost every one, so a sharded
/// map's per-CPU allocation would be paid for nothing. The lock is only ever taken on the refusal
/// path.
#[derive(Default)]
pub struct DroppedGeometries {
    entries: Mutex<IndexMap<DroppedGeometry, u64>>,
}

impl DroppedGeometries {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> DroppedGeometries {
        DroppedGeometries::default()
    }

    /// Records one refusal, counting a repeat of one already seen.
    pub fn record(&self, attribute: &NameBuf, refusal: GeometryError) {
        let dropped = DroppedGeometry {
            attribute: attribute.clone(),
            refusal,
        };

        *self.entries.lock().entry(dropped).or_insert(0) += 1;
    }

    /// Whether nothing was refused, which is the common case for a batch.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    /// Every distinct refusal and how often it happened, in the order the refusals first occurred.
    #[must_use]
    pub fn into_entries(self) -> Vec<(DroppedGeometry, u64)> {
        self.entries.into_inner().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::dropped_geometries::DroppedGeometries;
    use cassiopeia_geometry::{error::GeometryError, geometry::GeometryKind};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn ambiguous() -> GeometryError {
        GeometryError::AmbiguousMultiGeometry {
            origin: GeometryKind::MultiPolygon,
            members: 2,
        }
    }

    #[test]
    fn a_fresh_sink_holds_nothing() {
        assert!(DroppedGeometries::new().is_empty());
    }

    #[test]
    fn repeats_of_one_refusal_are_counted_rather_than_listed() {
        let sink = DroppedGeometries::new();
        sink.record(&name("location"), ambiguous());
        sink.record(&name("location"), ambiguous());
        sink.record(&name("location"), GeometryError::GeometryCollection);

        let entries = sink.into_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1, 2);
        assert_eq!(entries[1].1, 1);
    }

    #[test]
    fn the_entries_keep_the_order_the_refusals_first_occurred_in() {
        let sink = DroppedGeometries::new();
        sink.record(&name("area"), ambiguous());
        sink.record(&name("location"), ambiguous());

        let entries = sink.into_entries();
        assert_eq!(entries[0].0.attribute, name("area"));
        assert_eq!(entries[1].0.attribute, name("location"));
    }
}
