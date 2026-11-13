use crate::sensorthings::reading_keys::RenamedReading;
use cassiopeia_sensorthings::entity_id::EntityId;
use derive_more::Display;

/// How many renamed readings are kept to name; the rest are counted.
const NAMED_RENAMES: usize = 3;

/// How many of the other losses are kept to name: a report names the first and counts the rest.
const NAMED_FIRST: usize = 1;

/// The first few of one kind of loss, kept to name, and how many there were in all, so a source that
/// loses something on every line costs no more memory than one that loses it once.
#[derive(Debug, PartialEq, Eq)]
pub struct LossTally<T> {
    /// The first losses, in snapshot order.
    named: Vec<T>,
    /// How many losses are kept to name.
    limit: usize,
    /// How many losses there were, the named ones included.
    total: u64,
}

/// What reading a SensorThings snapshot lost or changed, for the run report.
#[derive(Debug, PartialEq, Eq)]
pub struct SensorThingsLosses {
    /// The data streams whose readings are keyed `<name>@<data stream id>`.
    renamed_readings: LossTally<RenamedReading>,
    /// The Things whose Location is no `GeoJSON`, so their records have no `location`.
    unreadable_locations: LossTally<UnreadableLocation>,
    /// The texts of the `phenomenonTime` values that named no instant, whose readings were dropped.
    unreadable_phenomenon_times: LossTally<Box<str>>,
}

/// A Thing whose Location is no `GeoJSON`, with the encoding it named.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
#[display("Thing {thing} {encoding}")]
pub struct UnreadableLocation {
    /// The Thing.
    pub thing: EntityId,
    /// What the Location states about its encoding.
    pub encoding: LocationEncoding,
}

/// What a Location states about how its `location` is encoded.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum LocationEncoding {
    /// The Location's `encodingType`.
    #[display("encodes it as `{_0}`")]
    Stated(Box<str>),
    /// The Location has no `encodingType`, though OGC 18-088 section 8.2.2 makes it mandatory.
    #[display("states no `encodingType`")]
    Unstated,
}

impl<T> LossTally<T> {
    /// An empty tally that names up to `limit` losses.
    const fn new(limit: usize) -> LossTally<T> {
        LossTally {
            named: Vec::new(),
            limit,
            total: 0,
        }
    }

    /// Counts one more loss, keeping it to name while fewer than the limit are kept.
    pub(super) fn record(&mut self, loss: T) {
        if self.named.len() < self.limit {
            self.named.push(loss);
        }
        self.total = self.total.saturating_add(1);
    }

    /// The first losses, in snapshot order.
    #[must_use]
    pub fn named(&self) -> &[T] {
        &self.named
    }

    /// How many losses there were in all.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// How many losses there were past the named ones, which a report counts rather than names.
    #[must_use]
    pub fn unnamed(&self) -> u64 {
        // A tally names at most its limit, which is small, so the named count fits a u64.
        self.total.saturating_sub(u64::try_from(self.named.len()).unwrap_or(u64::MAX))
    }
}

// Not derivable: each tally is built with how many of its losses a report names, which differs from
// one kind of loss to the next.
impl Default for SensorThingsLosses {
    /// Nothing lost yet.
    fn default() -> SensorThingsLosses {
        SensorThingsLosses {
            renamed_readings: LossTally::new(NAMED_RENAMES),
            unreadable_locations: LossTally::new(NAMED_FIRST),
            unreadable_phenomenon_times: LossTally::new(NAMED_FIRST),
        }
    }
}

impl SensorThingsLosses {
    /// The data streams whose readings are keyed `<name>@<data stream id>`.
    #[must_use]
    pub const fn renamed_readings(&self) -> &LossTally<RenamedReading> {
        &self.renamed_readings
    }

    /// The Things whose Location is no `GeoJSON`.
    #[must_use]
    pub const fn unreadable_locations(&self) -> &LossTally<UnreadableLocation> {
        &self.unreadable_locations
    }

    /// The `phenomenonTime` texts that named no instant.
    #[must_use]
    pub const fn unreadable_phenomenon_times(&self) -> &LossTally<Box<str>> {
        &self.unreadable_phenomenon_times
    }

    /// Counts a renamed reading.
    pub fn rename(&mut self, renamed: RenamedReading) {
        self.renamed_readings.record(renamed);
    }

    /// Counts a Thing whose Location is no `GeoJSON`.
    pub fn unreadable_location(&mut self, location: UnreadableLocation) {
        self.unreadable_locations.record(location);
    }

    /// Counts a reading dropped for its `phenomenonTime`, whose text is `text`.
    pub fn unreadable_phenomenon_time(&mut self, text: Box<str>) {
        self.unreadable_phenomenon_times.record(text);
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::ingest_losses::{LocationEncoding, LossTally, SensorThingsLosses, UnreadableLocation};
    use cassiopeia_sensorthings::entity_id::EntityId;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_tally_names_its_first_losses_and_counts_every_one() {
        let mut tally = LossTally::new(3);
        (0..5).for_each(|loss| tally.record(loss));

        assert_eq!(tally.named(), [0, 1, 2]);
        assert_eq!(tally.total(), 5);
        assert_eq!(tally.unnamed(), 2);
    }

    #[test]
    fn nothing_lost_names_and_counts_nothing() {
        let losses = SensorThingsLosses::default();

        assert_eq!(losses.renamed_readings().total(), 0);
        assert_eq!(losses.unreadable_locations().named(), []);
        assert_eq!(losses.unreadable_phenomenon_times().unnamed(), 0);
    }

    #[test]
    fn only_the_first_unreadable_time_is_named() {
        let mut losses = SensorThingsLosses::default();
        losses.unreadable_phenomenon_time("2026-11-12".into());
        losses.unreadable_phenomenon_time("soon".into());

        assert_eq!(losses.unreadable_phenomenon_times().named(), [Box::<str>::from("2026-11-12")]);
        assert_eq!(losses.unreadable_phenomenon_times().total(), 2);
    }

    #[test]
    fn an_unreadable_location_names_its_thing_and_encoding() {
        let stated = UnreadableLocation {
            thing: EntityId::Integer(12512),
            encoding: LocationEncoding::Stated("text/plain".into()),
        };
        let unstated = UnreadableLocation {
            thing: EntityId::Text("t-1".into()),
            encoding: LocationEncoding::Unstated,
        };

        assert_eq!(stated.to_string(), "Thing 12512 encodes it as `text/plain`");
        assert_eq!(unstated.to_string(), "Thing t-1 states no `encodingType`");
    }
}
