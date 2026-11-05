use crate::entity::name::NameBuf;
use std::fmt;

/// Where one attribute sits in an entity, written as an NGSI-LD attribute path (ETSI GS CIM 009
/// v1.9.1 clause 4.9): the top-level attribute, the sub-attributes beneath it separated by dots,
/// then any members of a structured value in one trailing pair of square brackets, such as
/// `temperature.accuracy` or `address[street.name]`.
///
/// A sub-attribute's own name says nothing about which attribute it qualifies, and two attributes
/// can each carry a sub-attribute of the same name, so anything met beneath the top level is named
/// by the whole path. The path is never empty: it always starts at a top-level attribute. Members
/// always come last, as clause 4.9 requires, because a member of a structured value carries no
/// sub-attributes of its own.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AttributePath {
    /// The top-level attribute the path starts at.
    root: NameBuf,
    /// The sub-attributes beneath the root, outermost first.
    sub_attributes: Vec<NameBuf>,
    /// The members of a structured value beneath the last sub-attribute, outermost first.
    members: Vec<NameBuf>,
}

impl AttributePath {
    /// The path from the top-level attribute `root` down through `sub_attributes`, then into the
    /// `members` of a structured value, each outermost first.
    #[must_use]
    pub const fn new(root: NameBuf, sub_attributes: Vec<NameBuf>, members: Vec<NameBuf>) -> AttributePath {
        AttributePath { root, sub_attributes, members }
    }

    /// The path of the top-level attribute `root` itself.
    #[must_use]
    pub const fn top_level(root: NameBuf) -> AttributePath {
        AttributePath::new(root, Vec::new(), Vec::new())
    }

    /// The top-level attribute the path starts at.
    #[must_use]
    pub const fn root(&self) -> &NameBuf {
        &self.root
    }
}

// Manual because the path joins a variable number of names in two separators, which no derive
// format string can express.
impl fmt::Display for AttributePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.root)?;
        self.sub_attributes.iter().try_for_each(|name| write!(f, ".{name}"))?;
        if let Some((first, rest)) = self.members.split_first() {
            write!(f, "[{first}")?;
            rest.iter().try_for_each(|name| write!(f, ".{name}"))?;
            f.write_str("]")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::{attribute_path::AttributePath, name::NameBuf};
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn a_top_level_attribute_is_its_own_path() {
        let path = AttributePath::top_level(name("category"));

        assert_eq!(path, AttributePath::new(name("category"), Vec::new(), Vec::new()));
        assert_eq!(path.to_string(), "category");
        assert_eq!(path.root(), &name("category"));
    }

    #[test]
    fn sub_attributes_are_named_from_the_top_level_attribute_down_separated_by_dots() {
        let path = AttributePath::new(name("temperature"), vec![name("accuracy"), name("level")], Vec::new());

        assert_eq!(path.to_string(), "temperature.accuracy.level");
        assert_eq!(path.root(), &name("temperature"));
    }

    #[test]
    fn members_of_a_structured_value_follow_in_one_trailing_pair_of_brackets() {
        let path = AttributePath::new(name("temperature"), vec![name("reading")], vec![name("origin"), name("kind")]);

        assert_eq!(path.to_string(), "temperature.reading[origin.kind]");
    }

    #[test]
    fn a_member_of_a_top_level_attribute_is_bracketed_right_after_it() {
        let path = AttributePath::new(name("address"), Vec::new(), vec![name("street")]);

        assert_eq!(path.to_string(), "address[street]");
    }
}
