//! The cursor store of incremental SensorThings reads.
//!
//! An incremental read resumes each data stream where the last successful cycle stopped, so it keeps
//! a read position per data stream across runs: the latest reading time written, the Observations
//! written within the lookback behind it, and when discovery last listed the data stream.
//! SensorThings gives no entity an insertion time (OGC 18-088 section 8.1, Table 2), so a late upload
//! is told apart from a reading already written only by its id.
//!
//! Each cursor is one redb database that records the kind of input it belongs to, its format and the
//! service root its positions were read from, and an open store holds its file exclusively. What a
//! cycle read forms an update, a value that changes nothing until it is committed in one write
//! transaction, so a cycle that fails leaves its cursors where they were. The store neither reads a
//! server nor writes output: the collector gathers what a cycle wrote, and the pipeline commits it.
//! The command line lists the stores of a cursors folder, shows their positions and resets them.

pub mod cursor_store;
pub mod cursor_store_error;
pub mod cursor_update;
pub mod cursors_folder;
pub mod position;
pub mod positions;
mod positions_table;
pub mod reset;
pub mod seen_observations;
mod store_metadata;
#[cfg(test)]
mod test_metadata;
