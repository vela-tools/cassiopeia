//! Reading `${NAME}` environment variables into the manifest fields that carry credentials: an
//! input's `source` and `headers`, and the output's `url` and `headers`.
//!
//! The values exist only in those fields. A mapping never sees them, they never join `vars`, and
//! each one is recorded as a secret the moment it is read.

pub mod error;
pub mod field;
pub mod resolution;
pub mod templated;
