use crate::{observed_property_name::ObservedPropertyName, offered_readings::unit_resolution::UnitResolution};
use getset::{CopyGetters, Getters};
use std::collections::BTreeSet;
use typed_builder::TypedBuilder;

/// One reading a server offers: the Datastreams whose `ObservedProperty` and the `MultiDatastream`
/// elements whose `ObservedProperty` has one name, which a mapping reads as `readings.<name>`.
#[derive(Debug, Clone, PartialEq, Eq, Getters, CopyGetters, TypedBuilder)]
pub struct OfferedReading {
    /// The `ObservedProperty` name the data streams share.
    #[getset(get = "pub")]
    name: ObservedPropertyName,

    /// The distinct texts the units of the reading are recognised by, as the server writes them; a
    /// unit without text adds none.
    #[getset(get = "pub")]
    units: BTreeSet<Box<str>>,

    /// What `cefact_unit` makes of each distinct unit; a unit without text adds nothing, since the
    /// filter yields nothing for it.
    #[getset(get = "pub")]
    codes: BTreeSet<UnitResolution>,

    /// The number of Datastreams of the name.
    #[getset(get_copy = "pub")]
    datastreams: usize,

    /// The number of `MultiDatastreams` with an element of the name.
    #[getset(get_copy = "pub")]
    multi_datastreams: usize,

    /// The number of Things that hold more than one reading of the name, whose extra readings a
    /// mapping reads under `<name>@<data stream id>` or `<name>@<MultiDatastream id>/<position>` keys.
    #[getset(get_copy = "pub")]
    things_with_repeats: usize,
}
