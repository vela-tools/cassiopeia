//! The readings a server offers: one per `ObservedProperty` name, with the units its data streams
//! use, what `cefact_unit` makes of them, and how many Things hold more than one data stream of the
//! name, so a mapping author knows which `readings.<key>` a record will carry.

pub mod error;
pub mod offered_reading;
pub mod reading_catalog;
pub mod unit_resolution;
