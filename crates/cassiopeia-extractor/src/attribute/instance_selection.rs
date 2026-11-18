/// Which of a top-level attribute's declared instances one resolution renders.
///
/// The instances of one attribute can be observed at distinct times and kept from distinct records
/// (ETSI GS CIM 009 v1.9.1 clause 4.5.5.3 resolves each instance on its own). Each record then
/// renders only the instances kept from it, so nothing an instance that was not kept from it holds
/// is read, or reported, off it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstanceSelection<'a> {
    /// Every declared instance.
    All,
    /// The instances declared at these positions.
    Only(&'a [usize]),
}

impl InstanceSelection<'_> {
    /// Whether the instance declared at `instance` is rendered.
    pub(crate) fn selects(self, instance: usize) -> bool {
        match self {
            InstanceSelection::All => true,
            InstanceSelection::Only(selected) => selected.contains(&instance),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance_selection::InstanceSelection;

    #[test]
    fn every_instance_is_rendered_unless_some_are_named() {
        assert!(InstanceSelection::All.selects(7));
        assert!(InstanceSelection::Only(&[0, 2]).selects(2));
        assert!(!InstanceSelection::Only(&[0, 2]).selects(1));
    }
}
