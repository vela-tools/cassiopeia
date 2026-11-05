use crate::attribute_path::AttributePath;
use cassiopeia_batch_sink::{batch_sink::BatchSink, counted_example::CountedExample};
use cassiopeia_geometry::wgs84::OutOfRangePosition;
use derive_more::{Deref, Into};

/// The geometries one batch dropped because a position lay outside WGS 84, one record per
/// declaration.
///
/// The declaration is the failure; the position differs from record to record, so the first one is
/// kept as its example, quoted with where it sits in its geometry, rather than keyed on. A source in
/// an undeclared reference system puts every one of its geometries out of range, and that is one
/// problem to report, not one per record.
#[derive(Default, Deref, Into)]
pub struct OutOfRangeGeometries(BatchSink<AttributePath, CountedExample>);

impl OutOfRangeGeometries {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> OutOfRangeGeometries {
        OutOfRangeGeometries::default()
    }

    /// Records that the declaration at `attribute` carried a geometry with `position` outside WGS
    /// 84.
    ///
    /// The first position a declaration is refused for is kept as its example; every later one is
    /// counted against that same record.
    pub fn record(&self, attribute: AttributePath, position: &OutOfRangePosition) {
        self.0.tally_owned(attribute, || CountedExample::first(&position.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_path::AttributePath, out_of_range_geometries::OutOfRangeGeometries};
    use cassiopeia_batch_sink::{batch_sink::BatchSink, counted_example::CountedExample, recurring::Recurring};
    use cassiopeia_geometry::{
        geometry::NgsiLdGeometry,
        wgs84::{OutOfRangePosition, check_range},
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;

    fn path(root: &str, nested: &[&str]) -> AttributePath {
        let name = |value: &str| NameBuf::new(value).expect("valid name");
        AttributePath::new(name(root), nested.iter().copied().map(name).collect(), Vec::new())
    }

    fn outside(longitude: f64) -> OutOfRangePosition {
        let point = NgsiLdGeometry::Point {
            coordinates: [longitude, 0.0].into(),
        };

        check_range(&point).expect_err("the longitude is outside WGS 84")
    }

    fn entries(sink: OutOfRangeGeometries) -> Vec<(AttributePath, CountedExample)> {
        BatchSink::from(sink).into_entries().collect()
    }

    #[test]
    fn a_fresh_sink_holds_nothing() {
        assert!(OutOfRangeGeometries::new().is_empty());
    }

    #[test]
    fn every_position_one_declaration_is_refused_for_is_counted_against_one_record_keeping_the_first() {
        let sink = OutOfRangeGeometries::new();
        sink.record(path("location", &[]), &outside(2600.5));
        sink.record(path("location", &[]), &outside(-500.0));

        let entries = entries(sink);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, path("location", &[]));
        assert_eq!(
            entries[0].1.example.as_ref(),
            "the position `[2600.5, 0]` at `coordinates` has a longitude outside WGS 84"
        );
        assert_eq!(entries[0].1.occurrences().get(), 2);
    }

    #[test]
    fn a_sub_attribute_is_a_record_apart_from_a_top_level_attribute_of_the_same_name() {
        let sink = OutOfRangeGeometries::new();
        sink.record(path("location", &[]), &outside(200.0));
        sink.record(path("observation", &["location"]), &outside(300.0));

        assert_eq!(entries(sink).len(), 2);
    }
}
