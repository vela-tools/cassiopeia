use cassiopeia_geometry::error::GeometryError;
use cassiopeia_ngsi_ld::entity::attribute_path::AttributePath;
use derive_more::Display;

/// One attribute, sub-attribute, or instance dropped because the geometry its source carried could not become the geometry its
/// mapping asked for.
///
/// The declaration's path and the refusal together are the whole diagnostic, and they are also its
/// identity: every record refused for the same reason on the same declaration produces an equal value,
/// so a run reports the problem once and counts how often it happened rather than repeating a line
/// per record.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
#[display("Attribute `{attribute}` dropped: {refusal}")]
pub struct DroppedGeometry {
    /// Where the declaration that lost its value sits in the mapping.
    pub attribute: AttributePath,
    /// Why the conversion was refused.
    pub refusal: GeometryError,
}

#[cfg(test)]
mod tests {
    use crate::dropped_geometry::DroppedGeometry;
    use cassiopeia_geometry::{error::GeometryError, geometry::GeometryKind};
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
    use pretty_assertions::{assert_eq, assert_ne};

    fn dropped(root: &str, nested: &[&str]) -> DroppedGeometry {
        let name = |value: &str| NameBuf::new(value).expect("valid name");
        DroppedGeometry {
            attribute: AttributePath::new(name(root), nested.iter().copied().map(name).collect(), Vec::new()),
            refusal: GeometryError::Uncoercible {
                origin: GeometryKind::Polygon,
                target: GeometryKind::Point,
            },
        }
    }

    #[test]
    fn the_message_names_a_top_level_attribute_and_the_reason() {
        assert_eq!(
            dropped("location", &[]).to_string(),
            "Attribute `location` dropped: a Polygon cannot become a Point without a declared `geometry: { convert: ... }` conversion"
        );
    }

    #[test]
    fn the_message_names_a_sub_attribute_by_its_path_from_the_top_level_attribute() {
        assert_eq!(
            dropped("temperature", &["footprint"]).to_string(),
            "Attribute `temperature.footprint` dropped: a Polygon cannot become a Point without a declared `geometry: { convert: ... }` conversion"
        );
    }

    #[test]
    fn two_records_refused_the_same_way_on_the_same_declaration_are_one_diagnostic() {
        assert_eq!(dropped("location", &[]), dropped("location", &[]));
        assert_ne!(dropped("location", &[]), dropped("area", &[]));
        assert_ne!(dropped("temperature", &["footprint"]), dropped("humidity", &["footprint"]));
    }
}
