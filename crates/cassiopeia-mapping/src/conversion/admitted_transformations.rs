use crate::transformation::Transformation;
use std::fmt;

/// The transformations one declaration admits, in declaration order, rendered as the fix a load
/// error offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedTransformations(Vec<Transformation>);

impl AdmittedTransformations {
    /// Holds `transformations`, the ones a declaration admits.
    #[must_use]
    pub const fn new(transformations: Vec<Transformation>) -> AdmittedTransformations {
        AdmittedTransformations(transformations)
    }

    /// The admitted transformations, in declaration order.
    #[must_use]
    pub fn as_slice(&self) -> &[Transformation] {
        &self.0
    }
}

// Manual because the fix reads differently when nothing is admitted, which no derive expresses.
impl fmt::Display for AdmittedTransformations {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some((first, rest)) = self.0.split_first() else {
            return formatter.write_str("remove the transformation: this declaration takes none");
        };

        write!(formatter, "remove the transformation or use one of: \"{first}\"")?;
        rest.iter().try_for_each(|transformation| write!(formatter, ", \"{transformation}\""))
    }
}

#[cfg(test)]
mod tests {
    use crate::{conversion::admitted_transformations::AdmittedTransformations, transformation::Transformation};
    use pretty_assertions::assert_eq;

    #[test]
    fn admitted_transformations_render_as_the_tokens_to_use_instead() {
        let admitted = AdmittedTransformations::new(vec![Transformation::Array, Transformation::Object, Transformation::Json]);

        assert_eq!(admitted.to_string(), r#"remove the transformation or use one of: "array", "object", "json""#);
    }

    #[test]
    fn no_admitted_transformation_renders_as_removing_it() {
        assert_eq!(
            AdmittedTransformations::new(Vec::new()).to_string(),
            "remove the transformation: this declaration takes none"
        );
    }
}
