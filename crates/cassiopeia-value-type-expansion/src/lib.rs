//! Expands the `valueType` of NGSI-LD attributes to the datatype IRIs they name.
//!
//! A `valueType` is written as a term, a compact IRI or an absolute IRI, and is type coerced into a
//! datatype IRI against the `@context` its entity is sent with (ETSI GS CIM 009 v1.9.1 clause 4.5.2).
//! Two spellings name the same datatype only when they expand to the same IRI, so a check that asks
//! whether a declared `valueType` names, say, a `DateTime` has to expand it the way a broker will:
//! against the entity's user `@context`, with the core `@context` in the last position (clause 4.4).

pub mod context_load_error;
pub mod context_loader;
pub mod core_context;
pub mod error;
pub mod expanded_value_types;
pub mod value_type_expander;
