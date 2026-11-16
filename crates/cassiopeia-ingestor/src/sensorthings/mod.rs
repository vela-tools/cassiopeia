//! Reading a SensorThings snapshot file into records.
//!
//! A snapshot holds the Datastreams a read discovered, each with its Thing, the Thing's Locations,
//! its `ObservedProperty` and its `Sensor`, and the Observations the read took, each with its
//! `FeatureOfInterest` when the read expanded it. A record joins a reading with all of these, so a
//! mapping reads one documented shape: `server`, `thing`, `location` and `readings`.

mod encoded_geometry;
pub mod error;
mod feature_of_interest;
pub mod ingest_losses;
pub mod ingestor;
mod location;
mod null_members;
pub mod reading_keys;
mod record_builder;
mod sensor;
mod served_id;
mod thing_index;
