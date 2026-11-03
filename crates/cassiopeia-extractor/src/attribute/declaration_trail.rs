use cassiopeia_ngsi_ld::entity::name::NameBuf;

/// How the declarations beneath one frame relate to the declaration it descended through.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Nesting {
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
pub(crate) struct DeclarationTrail<'a> {
    /// The declaration this frame descended through.
    name: &'a NameBuf,
    /// How the declarations beneath this one are nested in it.
    nesting: Nesting,
    /// The frame of the declaration holding this one, or `None` for a top-level attribute.
    parent: Option<&'a DeclarationTrail<'a>>,
}

impl<'a> DeclarationTrail<'a> {
    /// The frame for descending through `name` into its `nesting`, held beneath `parent`.
    pub(crate) const fn below(name: &'a NameBuf, nesting: Nesting, parent: Option<&'a DeclarationTrail<'a>>) -> DeclarationTrail<'a> {
        DeclarationTrail { name, nesting, parent }
    }

    /// The declaration this frame descended through.
    pub(crate) const fn name(&self) -> &'a NameBuf {
        self.name
    }

    /// How the declarations beneath this one are nested in it.
    pub(crate) const fn nesting(&self) -> Nesting {
        self.nesting
    }

    /// The frame of the declaration holding this one.
    pub(crate) const fn parent(&self) -> Option<&'a DeclarationTrail<'a>> {
        self.parent
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::declaration_trail::{DeclarationTrail, Nesting};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;

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
}
