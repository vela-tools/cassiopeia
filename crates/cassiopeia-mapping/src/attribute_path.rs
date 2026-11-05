use crate::attribute::declaration_trail::{DeclarationTrail, Nesting};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use std::{fmt, iter};

/// Where one declaration sits in a mapping, written as an NGSI-LD attribute path (ETSI GS CIM 009
/// v1.9.1 clause 4.9): the top-level attribute, the sub-attributes beneath it separated by dots,
/// then any members of a structured value in one trailing pair of square brackets, such as
/// `temperature.accuracy` or `address[street.name]`.
///
/// A sub-attribute's own name says nothing about which attribute it qualifies, and two attributes
/// can each carry a sub-attribute of the same name, so a loss met beneath the top level is named by
/// the whole path. The path is never empty: it always starts at a top-level attribute. Members
/// always come last, as clause 4.9 requires, because a member of a structured value declares no
/// sub-attributes of its own.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AttributePath {
    /// The top-level attribute the declaration sits under, or is.
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

    /// The path of `leaf`, resolved beneath the declarations `owners` holds.
    #[must_use]
    pub fn beneath(owners: Option<&DeclarationTrail<'_>>, leaf: &NameBuf) -> AttributePath {
        // The trail runs innermost first; reversed, it runs from the top-level attribute down, and
        // each frame says how the next name is nested in it.
        let mut frames = iter::successors(owners, |frame| frame.parent()).collect::<Vec<&DeclarationTrail<'_>>>();
        frames.reverse();
        let Some((root, held)) = frames.split_first() else {
            return AttributePath::new(leaf.clone(), Vec::new(), Vec::new());
        };

        // The path owns its names because a sink keeps it past the record it was met on; only a
        // refused value pays for this.
        let mut path = AttributePath::new(root.name().clone(), Vec::new(), Vec::new());
        let names = held.iter().map(|frame| frame.name()).chain(iter::once(leaf));
        let nestings = frames.iter().map(|frame| frame.nesting());
        for (name, nesting) in names.zip(nestings) {
            match nesting {
                Nesting::SubAttribute => path.sub_attributes.push(name.clone()),
                Nesting::Member => path.members.push(name.clone()),
            }
        }
        path
    }

    /// The top-level attribute the declaration sits under, or is.
    #[must_use]
    pub const fn root(&self) -> &NameBuf {
        &self.root
    }

    /// The top-level attribute the declaration sits under, or is, taken out of the path.
    #[must_use]
    pub fn into_root(self) -> NameBuf {
        self.root
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
    use crate::{
        attribute::declaration_trail::{DeclarationTrail, Nesting},
        attribute_path::AttributePath,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn a_top_level_attribute_is_its_own_path() {
        let path = AttributePath::beneath(None, &name("category"));

        assert_eq!(path, AttributePath::new(name("category"), Vec::new(), Vec::new()));
        assert_eq!(path.to_string(), "category");
        assert_eq!(path.root(), &name("category"));
    }

    #[test]
    fn sub_attributes_are_named_from_the_top_level_attribute_down_separated_by_dots() {
        let (temperature, accuracy) = (name("temperature"), name("accuracy"));
        let outer = DeclarationTrail::below(&temperature, Nesting::SubAttribute, None);
        let inner = DeclarationTrail::below(&accuracy, Nesting::SubAttribute, Some(&outer));

        let path = AttributePath::beneath(Some(&inner), &name("level"));

        assert_eq!(path, AttributePath::new(name("temperature"), vec![name("accuracy"), name("level")], Vec::new()));
        assert_eq!(path.to_string(), "temperature.accuracy.level");
        assert_eq!(path.root(), &name("temperature"));
    }

    #[test]
    fn members_of_a_structured_value_follow_in_one_trailing_pair_of_brackets() {
        let (temperature, reading, origin) = (name("temperature"), name("reading"), name("origin"));
        let outer = DeclarationTrail::below(&temperature, Nesting::SubAttribute, None);
        let middle = DeclarationTrail::below(&reading, Nesting::Member, Some(&outer));
        let inner = DeclarationTrail::below(&origin, Nesting::Member, Some(&middle));

        let path = AttributePath::beneath(Some(&inner), &name("kind"));

        assert_eq!(path.to_string(), "temperature.reading[origin.kind]");
    }

    #[test]
    fn a_member_of_a_top_level_attribute_is_bracketed_right_after_it() {
        let address = name("address");
        let frame = DeclarationTrail::below(&address, Nesting::Member, None);

        assert_eq!(AttributePath::beneath(Some(&frame), &name("street")).to_string(), "address[street]");
    }

    #[test]
    fn taking_the_root_out_of_a_path_yields_its_top_level_attribute() {
        let path = AttributePath::new(name("temperature"), vec![name("accuracy")], vec![name("value")]);

        assert_eq!(path.into_root(), name("temperature"));
    }
}
