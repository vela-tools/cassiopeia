//! The snapshot file: what a SensorThings read collected, as UTF-8 JSON Lines.
//!
//! Line 1 is the [`header::SnapshotHeader`]. Every data stream the read discovered follows, one per
//! line, its Datastreams and then its `MultiDatastreams`, and in a range or incremental read every
//! Observation follows them, one per line, grouped by data stream. Everything under `datastream`,
//! `multiDatastream` and `observation` is the server's JSON as sent, apart from the id an observation
//! line names its data stream by; the rest is Cassiopeia's own format, with camelCase keys and
//! kebab-case values.

pub mod error;
pub mod header;
pub mod line;
pub mod line_number;
pub mod reader;
pub mod writer;
