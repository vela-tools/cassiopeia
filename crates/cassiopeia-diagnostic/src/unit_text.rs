use derive_more::{AsRef, Display, From};

/// The text a mapping gave as an attribute's `unitCode` that is no UN/CEFACT Recommendation No. 20
/// code.
///
/// ETSI GS CIM 009 v1.9.1 clause 4.5.2 admits only a Recommendation No. 20 common code as a
/// `unitCode`. Text that is not one has no structure Cassiopeia knows of: it may be a symbol such as
/// `ug/m3`, a typo such as `GQQ`, or something else entirely, so it is carried as a newtype rather
/// than parsed.
#[derive(AsRef, Clone, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
#[as_ref(str)]
pub struct UnitText(Box<str>);

impl UnitText {
    /// Builds a unit text from the text the mapping gave.
    #[must_use]
    pub fn new(text: impl Into<Box<str>>) -> UnitText {
        UnitText(text.into())
    }

    /// The text as given.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::unit_text::UnitText;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_unit_text_renders_the_text_it_was_given() {
        assert_eq!(UnitText::new("ug/m3").as_str(), "ug/m3");
        assert_eq!(UnitText::new("ug/m3").to_string(), "ug/m3");
    }
}
