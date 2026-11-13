//! The snapshot file: what a SensorThings read collected, as UTF-8 JSON Lines.
//!
//! Line 1 is the [`header::SnapshotHeader`]. Every Datastream the read discovered follows, one per
//! line, and in a range or incremental read every Observation follows them, one per line, grouped by
//! data stream. Everything under `datastream` and `observation` is the server's JSON as sent; the
//! rest is Cassiopeia's own format, with camelCase keys and kebab-case values.

pub mod error;
pub mod header;
pub mod line;
pub mod line_number;
pub mod reader;
pub mod writer;
