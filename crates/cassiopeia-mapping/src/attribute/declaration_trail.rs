use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
use std::iter;

/// How the declarations beneath one frame relate to the declaration it descended through.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Nesting {
    /// They are its sub-attributes, declared under `properties`.
    SubAttribute,
    /// They are members of its structured value, declared under `mappings`.
    Member,
}

/// The declarations that hold the one being resolved, innermost first, as borrowed frames on the
/// resolver's own stack.
///
/// Resolution descends into sub-attributes and nested mappings by recursion, so each level keeps
/// one frame naming the declaration it descended through and how the declarations beneath it are
/// nested. Nothing is allocated on the way down: a path is built from the frames only when a loss
/// has to be named.
#[derive(Clone, Copy, Debug)]
pub struct DeclarationTrail<'a> {
    /// The declaration this frame descended through.
    name: &'a NameBuf,
    /// How the declarations beneath this one are nested in it.
    nesting: Nesting,
    /// The frame of the declaration holding this one, or `None` for a top-level attribute.
    parent: Option<&'a DeclarationTrail<'a>>,
}

impl<'a> DeclarationTrail<'a> {
    /// The frame for descending through `name` into its `nesting`, held beneath `parent`.
    #[must_use]
    pub const fn below(name: &'a NameBuf, nesting: Nesting, parent: Option<&'a DeclarationTrail<'a>>) -> DeclarationTrail<'a> {
        DeclarationTrail { name, nesting, parent }
    }

    /// The declaration this frame descended through.
    #[must_use]
    pub const fn name(&self) -> &'a NameBuf {
        self.name
    }

    /// How the declarations beneath this one are nested in it.
    #[must_use]
    pub const fn nesting(&self) -> Nesting {
        self.nesting
    }

    /// The frame of the declaration holding this one.
    #[must_use]
    pub const fn parent(&self) -> Option<&'a DeclarationTrail<'a>> {
        self.parent
    }

    /// The path of `leaf`, a declaration held beneath the declarations `owners` frames, or a
    /// top-level attribute when `owners` is `None`.
    #[must_use]
    pub fn path_of(owners: Option<&DeclarationTrail<'_>>, leaf: &NameBuf) -> AttributePath {
        // The trail runs innermost first; reversed, it runs from the top-level attribute down, and
        // each frame says how the next name is nested in it.
        let mut frames = iter::successors(owners, |frame| frame.parent()).collect::<Vec<&DeclarationTrail<'_>>>();
        frames.reverse();
        let Some((root, held)) = frames.split_first() else {
            return AttributePath::top_level(leaf.clone());
        };

        // The path owns its names because a sink keeps it past the record it was met on; only a
        // refused value pays for this.
        let (mut sub_attributes, mut members) = (Vec::new(), Vec::new());
        let names = held.iter().map(|frame| frame.name()).chain(iter::once(leaf));
        let nestings = frames.iter().map(|frame| frame.nesting());
        for (name, nesting) in names.zip(nestings) {
            match nesting {
                Nesting::SubAttribute => sub_attributes.push(name.clone()),
                Nesting::Member => members.push(name.clone()),
            }
        }
        AttributePath::new(root.name().clone(), sub_attributes, members)
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::declaration_trail::{DeclarationTrail, Nesting};
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn a_frame_names_its_declaration_its_nesting_and_reaches_its_holder() {
        let (outer_name, inner_name) = (NameBuf::new("temperature").unwrap(), NameBuf::new("accuracy").unwrap());
        let outer = DeclarationTrail::below(&outer_name, Nesting::SubAttribute, None);
        let inner = DeclarationTrail::below(&inner_name, Nesting::Member, Some(&outer));

        assert_eq!(inner.name(), &inner_name);
        assert_eq!(inner.nesting(), Nesting::Member);
        assert_eq!(inner.parent().map(DeclarationTrail::name), Some(&outer_name));
        assert!(outer.parent().is_none());
    }

    #[test]
    fn a_declaration_beneath_no_frame_is_a_top_level_attribute() {
        assert_eq!(DeclarationTrail::path_of(None, &name("category")), AttributePath::top_level(name("category")));
    }

    #[test]
    fn sub_attributes_are_named_from_the_top_level_attribute_down() {
        let (temperature, accuracy) = (name("temperature"), name("accuracy"));
        let outer = DeclarationTrail::below(&temperature, Nesting::SubAttribute, None);
        let inner = DeclarationTrail::below(&accuracy, Nesting::SubAttribute, Some(&outer));

        assert_eq!(
            DeclarationTrail::path_of(Some(&inner), &name("level")),
            AttributePath::new(name("temperature"), vec![name("accuracy"), name("level")], Vec::new())
        );
    }

    #[test]
    fn members_beneath_a_sub_attribute_follow_it_as_members() {
        let (temperature, reading, origin) = (name("temperature"), name("reading"), name("origin"));
        let outer = DeclarationTrail::below(&temperature, Nesting::SubAttribute, None);
        let middle = DeclarationTrail::below(&reading, Nesting::Member, Some(&outer));
        let inner = DeclarationTrail::below(&origin, Nesting::Member, Some(&middle));

        assert_eq!(
            DeclarationTrail::path_of(Some(&inner), &name("kind")),
            AttributePath::new(name("temperature"), vec![name("reading")], vec![name("origin"), name("kind")])
        );
    }

    #[test]
    fn a_member_of_a_top_level_attribute_is_a_member_right_beneath_it() {
        let address = name("address");
        let frame = DeclarationTrail::below(&address, Nesting::Member, None);

        assert_eq!(
            DeclarationTrail::path_of(Some(&frame), &name("street")),
            AttributePath::new(name("address"), Vec::new(), vec![name("street")])
        );
    }
}
