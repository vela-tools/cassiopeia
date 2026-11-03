use crate::dropped_null::{DroppedNull, NullSite};
use cassiopeia_batch_sink::{batch_sink::BatchSink, occurrences::Occurrences};
use cassiopeia_ngsi_ld::{entity::name::NameBuf, value::omitted_nulls::OmittedNulls};
use derive_more::{Deref, Into};

/// The attribute values one batch wrote without the nulls their source held, one count per
/// attribute and site.
///
/// Each count is of attribute values, not of nulls: a record whose value lost three null items
/// counts once, the way every other dropped-attribute sink counts one record's loss once.
#[derive(Default, Deref, Into)]
pub struct DroppedNulls(BatchSink<DroppedNull, Occurrences>);

impl DroppedNulls {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> DroppedNulls {
        DroppedNulls::default()
    }

    /// Records the nulls one value of `attribute` was written without.
    pub fn record(&self, attribute: &NameBuf, omitted: OmittedNulls) {
        for (site, count) in [(NullSite::ArrayItem, omitted.array_items), (NullSite::ObjectMember, omitted.object_members)] {
            if count > 0 {
                // The key carries the site by value, so it is built owned; this runs only for a value
                // that lost a null.
                let dropped = DroppedNull {
                    attribute: attribute.clone(),
                    site,
                };
                self.0.tally_owned(dropped, Occurrences::first);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        dropped_null::{DroppedNull, NullSite},
        dropped_nulls::DroppedNulls,
    };
    use cassiopeia_batch_sink::{batch_sink::BatchSink, recurring::Recurring};
    use cassiopeia_ngsi_ld::{entity::name::NameBuf, value::omitted_nulls::OmittedNulls};
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn entries(sink: DroppedNulls) -> Vec<(DroppedNull, u64)> {
        BatchSink::from(sink)
            .into_entries()
            .map(|(dropped, record)| (dropped, record.occurrences().get()))
            .collect()
    }

    #[test]
    fn a_value_that_lost_nothing_records_nothing() {
        let sink = DroppedNulls::new();
        sink.record(&name("tags"), OmittedNulls::default());

        assert!(sink.is_empty());
    }

    #[test]
    fn a_value_counts_once_per_site_however_many_nulls_it_lost_there() {
        let sink = DroppedNulls::new();
        sink.record(
            &name("tags"),
            OmittedNulls {
                array_items: 3,
                object_members: 0,
            },
        );
        sink.record(
            &name("tags"),
            OmittedNulls {
                array_items: 1,
                object_members: 2,
            },
        );

        assert_eq!(
            entries(sink),
            [
                (
                    DroppedNull {
                        attribute: name("tags"),
                        site: NullSite::ArrayItem,
                    },
                    2
                ),
                (
                    DroppedNull {
                        attribute: name("tags"),
                        site: NullSite::ObjectMember,
                    },
                    1
                ),
            ]
        );
    }
}
