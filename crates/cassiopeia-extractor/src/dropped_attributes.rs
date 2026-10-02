use crate::{dropped_geometries::DroppedGeometries, unresolved_templates::UnresolvedTemplates};
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;

/// The attributes one batch's extraction dropped while keeping their entities, one sink per reason.
///
/// Each reason is a different problem with a different fix, so each keeps its own sink and is
/// reported under its own code; they travel together because every one of them is the same kind of
/// loss, met in the same stage, over the same batch.
#[derive(Default)]
pub struct DroppedAttributes {
    /// Geometries the declared type cannot hold, or that this mapping did not authorise converting.
    pub geometries: DroppedGeometries,
    /// Attribute values that read as no supported spelling of a date-time.
    pub timestamps: UnreadableTimestamps,
    /// Attributes whose template could not be resolved against the record.
    pub templates: UnresolvedTemplates,
}

impl DroppedAttributes {
    /// Opens a set of empty sinks.
    #[must_use]
    pub fn new() -> DroppedAttributes {
        DroppedAttributes::default()
    }
}

#[cfg(test)]
mod tests {
    use crate::dropped_attributes::DroppedAttributes;

    #[test]
    fn a_fresh_set_of_sinks_holds_nothing() {
        let dropped = DroppedAttributes::new();

        assert!(dropped.geometries.is_empty());
        assert!(dropped.timestamps.is_empty());
        assert!(dropped.templates.is_empty());
    }
}
