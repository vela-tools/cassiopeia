//! Reading a SensorThings snapshot file into records.
//!
//! A snapshot holds the data streams a read discovered, Datastreams and `MultiDatastreams`, each with
//! its Thing, the Thing's Locations, what it observes and its `Sensor`, and the Observations the read
//! took, each with its `FeatureOfInterest` when the read expanded it. A Datastream gives each of its
//! Observations one reading, and a `MultiDatastream` one per element, by position. A record joins
//! readings with all of these, so a mapping reads one documented shape: `server`, `thing`,
//! `location` and `readings`.

pub mod element_position;
mod encoded_geometry;
pub mod error;
mod feature_of_interest;
pub mod ingest_losses;
pub mod ingestor;
mod location;
mod null_members;
mod observation_readings;
pub mod reading_keys;
mod record_builder;
mod sensor;
mod served_id;
mod stream_readings;
mod thing_index;
