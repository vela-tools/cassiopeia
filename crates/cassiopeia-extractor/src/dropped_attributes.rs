use crate::{
    dropped_geometries::DroppedGeometries,
    dropped_nulls::DroppedNulls,
    out_of_range_geometries::OutOfRangeGeometries,
    refused_json_payloads::RefusedJsonPayloads,
    unusable_values::UnusableValues,
};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;
use cassiopeia_unresolved_templates::unresolved_templates::UnresolvedTemplates;

/// What one batch's extraction dropped while keeping its entities, one sink per reason: whole
/// attributes, and the nulls left out of attribute values.
///
/// Each reason is a different problem with a different fix, so each keeps its own sink and is
/// reported under its own code; they travel together because every one of them is the same kind of
/// loss, met in the same stage, over the same batch.
#[derive(Default)]
pub struct DroppedAttributes {
    /// Geometries the declared type cannot hold, or that this mapping did not authorise converting.
    pub geometries: DroppedGeometries,
    /// Geometries with a position outside WGS 84.
    pub out_of_range_geometries: OutOfRangeGeometries,
    /// Attribute values that read as no supported spelling of a date-time.
    pub timestamps: UnreadableTimestamps,
    /// Attributes whose template could not be resolved against the record.
    pub templates: UnresolvedTemplates<NameBuf>,
    /// `JsonProperty` values that are no JSON object or array of objects.
    pub json_payloads: RefusedJsonPayloads,
    /// `VocabProperty` and `LanguageProperty` values their kind cannot hold.
    pub unusable_values: UnusableValues,
    /// Attribute values written without the nulls their source held.
    pub nulls: DroppedNulls,
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
        assert!(dropped.out_of_range_geometries.is_empty());
        assert!(dropped.timestamps.is_empty());
        assert!(dropped.templates.is_empty());
        assert!(dropped.json_payloads.is_empty());
        assert!(dropped.unusable_values.is_empty());
        assert!(dropped.nulls.is_empty());
    }
}
