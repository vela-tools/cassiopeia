//! Reading a SensorThings snapshot file into records.
//!
//! A snapshot holds the Datastreams a read discovered, each with its Thing, the Thing's Locations
//! and its `ObservedProperty`, and the Observations the read took. A record joins a reading with all
//! of these, so a mapping reads one documented shape: `server`, `thing`, `location` and `readings`.

pub mod error;
pub mod ingest_losses;
pub mod ingestor;
mod location;
mod null_members;
pub mod reading_keys;
mod record_builder;
mod served_id;
mod thing_index;
