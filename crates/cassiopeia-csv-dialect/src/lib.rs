//! The CSV dialect: the parameters a specific CSV file is parsed with, how a file is opened under
//! them, and the part of them a manifest may state instead of leaving to detection.

pub mod character_encoding;
pub mod decimal_separator;
pub mod dialect;
pub mod dialect_byte;
pub mod error;
pub mod escape;
pub mod header_flag;
pub mod header_rows;
pub mod header_statement;
pub mod line_terminator;
pub mod misshapen_row;
pub mod null_token;
pub mod preamble;
pub mod quote;
mod record_end;
pub mod row_number;
pub mod statement;
pub mod value_notation;
