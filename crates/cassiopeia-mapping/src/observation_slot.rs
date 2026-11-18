/// One slot a current-state store ranks a mapping's records in: one distinct `observedAt` template
/// that a top-level attribute, or one of its instances, carries as its own.
///
/// Each slot keeps the record whose value for its template names the latest instant, so every
/// attribute it ranks comes from the record where that attribute was observed last (ETSI GS CIM 009
/// v1.9.1 clause 4.5.5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotIndex(usize);

impl SlotIndex {
    /// The slot at `index`.
    #[must_use]
    pub const fn new(index: usize) -> SlotIndex {
        SlotIndex(index)
    }

    /// The position of the slot.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// The slots ranking one top-level attribute: the attribute itself, or each of its instances.
///
/// A declaration or instance without an `observedAt` of its own has no slot: it comes from the
/// record observed last over every template of the mapping.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributeSlots {
    /// The slot of an attribute that declares no instances.
    declaration: Option<SlotIndex>,
    /// The slot of each declared instance, in declaration order.
    instances: Vec<Option<SlotIndex>>,
}

impl AttributeSlots {
    /// The slots of an attribute ranked as `declaration` and, when it declares instances, each of
    /// them as `instances` ranks it.
    #[must_use]
    pub const fn new(declaration: Option<SlotIndex>, instances: Vec<Option<SlotIndex>>) -> AttributeSlots {
        AttributeSlots { declaration, instances }
    }

    /// The slot ranking an attribute that declares no instances.
    #[must_use]
    pub const fn declaration(&self) -> Option<SlotIndex> {
        self.declaration
    }

    /// The slot ranking the instance declared at `instance`.
    #[must_use]
    pub fn instance(&self, instance: usize) -> Option<SlotIndex> {
        self.instances.get(instance).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use crate::observation_slot::{AttributeSlots, SlotIndex};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_slot_names_the_position_it_was_made_with() {
        assert_eq!(SlotIndex::new(1).get(), 1);
    }

    #[test]
    fn an_instance_beyond_those_declared_or_without_a_slot_has_none() {
        let slots = AttributeSlots::new(None, vec![Some(SlotIndex::new(0)), None]);

        assert_eq!(slots.instance(0), Some(SlotIndex::new(0)));
        assert_eq!(slots.instance(1), None);
        assert_eq!(slots.instance(2), None);
        assert_eq!(slots.declaration(), None);
    }
}
