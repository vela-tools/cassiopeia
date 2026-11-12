//! Unit notations resolved to the UN/CEFACT Recommendation No. 20 code NGSI-LD writes in `unitCode`.
//!
//! NGSI-LD records the unit of a `Property` value as a Recommendation No. 20 common code (ETSI GS CIM 009
//! v1.9.1 clause 4.5.2), but sources rarely send one. They send a symbol (`µg/m³`, `°C`), a UCUM code
//! (`ug/m3`, `Cel`), a UDUNITS code (`degC`), a QUDT unit IRI (`https://qudt.org/vocab/unit/MicroGM-PER-M3`),
//! or a unit name (`microgram per cubic metre`). This crate knows which notation means which code, so
//! that knowledge lives in one place for every component that needs it.
//!
//! [`resolve`](crate::resolution::resolve) tries a notation as a QUDT unit IRI, then as a symbol or code
//! of a unit system, then as a unit name, then as a Recommendation No. 20 code. It prefers a code current
//! in the latest revision of the recommendation, resolves to a deprecated code only for a unit that has no
//! current one, such as the hectare, and never resolves to a deleted code.
//!
//! The notations come from complete published sources, not from a hand-kept list:
//!
//! - every symbol and name Recommendation No. 20 itself gives, read from `cefact-units`;
//! - every QUDT unit that carries a `qudt:uneceCommonCode`, with its IRI, symbols, UCUM and UDUNITS codes,
//!   and English labels, generated from the QUDT unit vocabulary by `cassiopeia-unit-notation-generator`.
//!
//! What Cassiopeia adds by hand is limited to what those sources get wrong or leave open: corrections of
//! QUDT units whose published code denotes a different unit, a ruling for each notation the sources give
//! to more than one unit, and a few notations in common use that no source lists. Each is checked against
//! the sources by a test, so one a source update makes stale fails the build rather than lingering.

pub mod resolution;

mod candidates;
mod notation_index;
mod notation_key;
mod notation_sources;
mod qudt;
mod qudt_unit_iri;
mod ruling;
mod supplement;
