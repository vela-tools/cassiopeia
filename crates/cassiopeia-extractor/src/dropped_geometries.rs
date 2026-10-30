use crate::dropped_geometry::DroppedGeometry;
use cassiopeia_batch_sink::{batch_sink::BatchSink, occurrences::Occurrences};
use cassiopeia_geometry::error::GeometryError;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use derive_more::{Deref, Into};

/// The refused geometry conversions gathered while one batch of entities is extracted, one count
/// per distinct refusal of one attribute.
///
/// The [`DroppedGeometry`] is the whole diagnostic as well as its key, so a record holds nothing but
/// how often it was met.
#[derive(Default, Deref, Into)]
pub struct DroppedGeometries(BatchSink<DroppedGeometry, Occurrences>);

impl DroppedGeometries {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> DroppedGeometries {
        DroppedGeometries::default()
    }

    /// Records one refusal, counting a repeat of one already seen.
    pub fn record(&self, attribute: &NameBuf, refusal: GeometryError) {
        let dropped = DroppedGeometry {
            // The key carries the refusal by value, so it is built owned on every refusal; this runs
            // on the refusal path only.
            attribute: attribute.clone(),
            refusal,
        };

        self.0.tally_owned(dropped, Occurrences::first);
    }
}

#[cfg(test)]
mod tests {
    use crate::{dropped_geometries::DroppedGeometries, dropped_geometry::DroppedGeometry};
    use cassiopeia_batch_sink::{batch_sink::BatchSink, recurring::Recurring};
    use cassiopeia_geometry::{error::GeometryError, geometry::GeometryKind};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn entries(sink: DroppedGeometries) -> Vec<(DroppedGeometry, u64)> {
        BatchSink::from(sink)
            .into_entries()
            .map(|(dropped, record)| (dropped, record.occurrences().get()))
            .collect()
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

        let entries = entries(sink);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1, 2);
        assert_eq!(entries[1].1, 1);
    }

    #[test]
    fn the_entries_keep_the_order_the_refusals_first_occurred_in() {
        let sink = DroppedGeometries::new();
        sink.record(&name("area"), ambiguous());
        sink.record(&name("location"), ambiguous());

        let entries = entries(sink);
        assert_eq!(entries[0].0.attribute, name("area"));
        assert_eq!(entries[1].0.attribute, name("location"));
    }
}
