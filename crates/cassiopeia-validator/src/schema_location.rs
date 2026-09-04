use std::path::PathBuf;

/// Where an entity type's schema is found, and how a failure to load it is treated.
pub enum SchemaLocation {
    /// A schema the run explicitly requested for this type; a failure to load it is fatal.
    Explicit(PathBuf),
    /// A schema found by the Smart Data Models convention; its absence is a tolerated opt-out.
    Convention(PathBuf),
}
