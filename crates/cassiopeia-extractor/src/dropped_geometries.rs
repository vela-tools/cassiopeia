use crate::dropped_geometry::DroppedGeometry;
use cassiopeia_batch_sink::{batch_sink::BatchSink, occurrences::Occurrences};
use cassiopeia_geometry::error::GeometryError;
use cassiopeia_ngsi_ld::entity::attribute_path::AttributePath;
use derive_more::{Deref, Into};

/// The refused geometry conversions gathered while one batch of entities is extracted, one count
/// per distinct refusal of one declaration.
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

    /// Records that the declaration at `attribute` was refused for `refusal`, counting a repeat of
    /// one already seen.
    pub fn record(&self, attribute: AttributePath, refusal: GeometryError) {
        self.0.tally_owned(DroppedGeometry { attribute, refusal }, Occurrences::first);
    }
}

#[cfg(test)]
mod tests {
    use crate::{dropped_geometries::DroppedGeometries, dropped_geometry::DroppedGeometry};
    use cassiopeia_batch_sink::{batch_sink::BatchSink, recurring::Recurring};
    use cassiopeia_geometry::{error::GeometryError, geometry::GeometryKind};
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn top_level(value: &str) -> AttributePath {
        AttributePath::top_level(name(value))
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
        sink.record(top_level("location"), ambiguous());
        sink.record(top_level("location"), ambiguous());
        sink.record(top_level("location"), GeometryError::GeometryCollection);

        let entries = entries(sink);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1, 2);
        assert_eq!(entries[1].1, 1);
    }

    #[test]
    fn the_entries_keep_the_order_the_refusals_first_occurred_in() {
        let sink = DroppedGeometries::new();
        sink.record(top_level("area"), ambiguous());
        sink.record(top_level("location"), ambiguous());

        let entries = entries(sink);
        assert_eq!(entries[0].0.attribute, top_level("area"));
        assert_eq!(entries[1].0.attribute, top_level("location"));
    }

    #[test]
    fn one_sub_attribute_name_under_two_attributes_is_two_records() {
        let sink = DroppedGeometries::new();
        sink.record(AttributePath::new(name("temperature"), vec![name("footprint")], Vec::new()), ambiguous());
        sink.record(AttributePath::new(name("humidity"), vec![name("footprint")], Vec::new()), ambiguous());

        let entries = entries(sink);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0.attribute.to_string(), "temperature.footprint");
        assert_eq!(entries[1].0.attribute.to_string(), "humidity.footprint");
    }
}
