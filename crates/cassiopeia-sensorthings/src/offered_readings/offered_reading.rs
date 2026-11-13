use crate::{observed_property_name::ObservedPropertyName, offered_readings::unit_resolution::UnitResolution};
use getset::{CopyGetters, Getters};
use std::collections::BTreeSet;
use typed_builder::TypedBuilder;

/// One reading a server offers: the data streams whose `ObservedProperty` has one name, which a
/// mapping reads as `readings.<name>`.
#[derive(Debug, Clone, PartialEq, Eq, Getters, CopyGetters, TypedBuilder)]
pub struct OfferedReading {
    /// The `ObservedProperty` name the data streams share.
    #[getset(get = "pub")]
    name: ObservedPropertyName,

    /// The distinct texts the data streams' units are recognised by, as the server writes them; a
    /// data stream whose unit has no text adds none.
    #[getset(get = "pub")]
    units: BTreeSet<Box<str>>,

    /// What `cefact_unit` makes of each distinct unit; a data stream whose unit has no text adds
    /// nothing, since the filter yields nothing for it.
    #[getset(get = "pub")]
    codes: BTreeSet<UnitResolution>,

    /// The number of data streams of the name.
    #[getset(get_copy = "pub")]
    datastreams: usize,

    /// The number of Things that hold more than one data stream of the name, whose extra data
    /// streams a mapping reads under `<name>@<data stream id>` keys.
    #[getset(get_copy = "pub")]
    things_with_repeats: usize,
}
