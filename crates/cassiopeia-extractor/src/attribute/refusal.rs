use cassiopeia_geometry::error::GeometryError;
use derive_more::From;

/// Why an attribute could not take the value its source carried.
///
/// Neither refusal fails the record: the attribute drops, the entity is still emitted, and the run
/// names what it lost. They share one type because they arrive from one place (applying an
/// attribute's declared transformation) and are told apart again where each is bound to the sink
/// that gathers it.
#[derive(Clone, Debug, Eq, From, PartialEq)]
pub(crate) enum AttributeRefusal {
    /// The source carried a geometry the declared type cannot hold, or one this mapping did not
    /// authorise converting.
    Geometry(GeometryError),
    /// The source carried text that reads as no supported spelling of a date-time.
    #[from(ignore)]
    UnreadableTimestamp {
        /// The text that would not read, quoted back so a mapping author can see its shape.
        text: Box<str>,
    },
}

#[cfg(test)]
mod tests {
    use crate::attribute::refusal::AttributeRefusal;
    use cassiopeia_geometry::error::GeometryError;

    #[test]
    fn a_geometry_error_converts_into_a_refusal() {
        assert_eq!(
            AttributeRefusal::from(GeometryError::GeometryCollection),
            AttributeRefusal::Geometry(GeometryError::GeometryCollection)
        );
    }
}
