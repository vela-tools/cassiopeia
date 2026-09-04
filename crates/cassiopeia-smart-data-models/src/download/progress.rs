use crate::schema_id::SchemaId;

/// What a download reports as it runs.
///
/// Sent over a channel rather than handed to a callback, so the reporting side chooses its own
/// pace and the download never runs caller code on its own tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    /// The published lists are being read.
    ReadingPublishedLists,

    /// The lists have been read and this many schemas will be fetched.
    Planned {
        /// How many schemas the download expects to fetch.
        schemas: usize,
    },

    /// One schema has been fetched and stored.
    Stored {
        /// The schema that was stored.
        id: SchemaId,
    },

    /// One schema could not be fetched or stored.
    Failed {
        /// The schema that failed.
        id: SchemaId,
        /// A human-readable description of why it failed.
        reason: String,
    },

    /// Every schema has been attempted.
    Finished {
        /// How many schemas were stored.
        stored: usize,
        /// How many schemas could not be fetched or stored.
        failed: usize,
    },
}

#[cfg(test)]
mod tests {
    use crate::{download::progress::ProgressEvent, schema_id::SchemaId};
    use std::str::FromStr;

    #[test]
    fn events_of_the_same_shape_compare_equal() {
        let id = SchemaId::from_str("Point").unwrap();

        assert_eq!(ProgressEvent::Stored { id: id.clone() }, ProgressEvent::Stored { id });
        assert_ne!(ProgressEvent::ReadingPublishedLists, ProgressEvent::Planned { schemas: 1 });
    }
}
