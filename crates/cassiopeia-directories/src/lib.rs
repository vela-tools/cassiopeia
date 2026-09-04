//! Every default path a Cassiopeia installation uses, resolved once in one place.
//!
//! Two layers make up this crate. [`base`] answers the three XDG base questions (where
//! configuration, data, and state belong), applying the container override and the no-home
//! fallback. [`locations`] names the individual leaves an installation reads and writes (the schema
//! store, the mapping folder, the log directory, the configuration file) by classifying each onto
//! one of those bases. Centralising the resolution here keeps the container detection and the XDG
//! classification from being duplicated across the configuration and reporter crates.

pub mod base;
pub mod locations;
