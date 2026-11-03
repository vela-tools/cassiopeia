//! A CSV column as a record reads it: its one-based number, the key its values are read by, and the
//! rename that keeps two columns asking for one key apart.

pub mod column_key;
pub mod column_number;
pub mod renamed_column;
