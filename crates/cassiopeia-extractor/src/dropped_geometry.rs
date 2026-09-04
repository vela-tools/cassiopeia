use cassiopeia_geometry::error::GeometryError;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use derive_more::Display;

/// One attribute dropped because the geometry its source carried could not become the geometry its
/// mapping asked for.
///
/// The attribute name and the refusal together are the whole diagnostic, and they are also its
/// identity: every record refused for the same reason on the same attribute produces an equal value,
/// so a run reports the problem once and counts how often it happened rather than repeating a line
/// per record.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
#[display("Attribute `{attribute}` dropped: {refusal}")]
pub struct DroppedGeometry {
    /// The attribute that lost its value.
    pub attribute: NameBuf,
    /// Why the conversion was refused.
    pub refusal: GeometryError,
}

#[cfg(test)]
mod tests {
    use crate::dropped_geometry::DroppedGeometry;
    use cassiopeia_geometry::{error::GeometryError, geometry::GeometryKind};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;

    fn dropped(attribute: &str) -> DroppedGeometry {
        DroppedGeometry {
            attribute: NameBuf::new(attribute).expect("valid name"),
            refusal: GeometryError::AmbiguousMultiGeometry {
                origin: GeometryKind::MultiPolygon,
                members: 2,
            },
        }
    }

    #[test]
    fn the_message_names_the_attribute_and_the_reason() {
        let message = dropped("location").to_string();

        assert!(message.contains("location"), "{message}");
        assert!(message.contains("MultiPolygon"), "{message}");
    }

    #[test]
    fn two_records_refused_the_same_way_on_the_same_attribute_are_one_diagnostic() {
        assert_eq!(dropped("location"), dropped("location"));
        assert_ne!(dropped("location"), dropped("area"));
    }
}
