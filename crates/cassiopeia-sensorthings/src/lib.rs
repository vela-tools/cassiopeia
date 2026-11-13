//! The OGC SensorThings API vocabulary Cassiopeia shares across its stages.
//!
//! A SensorThings input crosses several stages: a collector that reads a server, the profiler that
//! recognizes what it wrote, and the ingestor that turns it into records. The command that lists the
//! readings a server offers and the `cefact_unit` template filter read the same Datastreams. Each of
//! them speaks of the same things, so they live here, free of I/O beyond the reader and writer handed
//! to the snapshot file: service roots and their versions (OGC 18-088 section 9.1), the settings a
//! remote input is read with, the requests a read sends and the pages it is answered with (section
//! 9.3), entity ids and their order, a Datastream's unit of measurement (section 8.2.4), the readings
//! a server offers, the instant a `phenomenonTime` stands for (section 8.2.7), the read modes, and the
//! snapshot file that is the boundary between transport and format.

pub mod entity_id;
pub mod observed_property_name;
pub mod offered_readings;
pub mod paging;
pub mod phenomenon_time;
pub mod read_kind;
pub mod request;
pub mod service;
pub mod service_root;
pub mod settings;
pub mod snapshot;
pub mod unit_of_measurement;
pub mod version;
