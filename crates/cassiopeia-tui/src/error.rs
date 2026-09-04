use std::{io, result};
use thiserror::Error;

/// A failure raised while a terminal screen is running.
#[derive(Debug, Error)]
pub enum TuiError {
    /// The terminal backend could not be set up, drawn to, or read from.
    #[error("The terminal could not be set up, drawn to, or read from")]
    Io(#[from] io::Error),

    /// A schema tree could not be assembled, because two sibling nodes claimed the same identifier.
    #[error("Failed to build schema tree: {0}")]
    Tree(String),
}

pub type Result<T, E = TuiError> = result::Result<T, E>;
