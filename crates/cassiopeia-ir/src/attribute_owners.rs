use cassiopeia_mapping::mapping::Mapping;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::sync::Arc;

/// Where one attribute declaration sits among the mappings an entity carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CarriedDeclaration {
    /// The index of the declaring mapping among the entity's carried mappings, in the order the
    /// entity carries them.
    pub mapping: usize,
    /// The index of the declaration among that mapping's attributes, in declaration order.
    pub declaration: usize,
}

/// Which declaration each attribute of an entity joined from several mappings is taken from.
///
/// Several mappings that contribute to one entity may each declare an attribute of the same name,
/// and the attribute is taken whole from one of them: its value, its declaration, and its
/// sub-attributes. The record holds one flag per declaration of every carried mapping, so it names
/// the owning declaration by position and never copies an attribute name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributeOwners {
    /// Where each carried mapping's flags start in `owned`, in carried order, followed by the total,
    /// so the flags of mapping `i` are `starts[i]..starts[i + 1]`.
    starts: SmallVec<[usize; 3]>,
    /// Whether each declaration owns the attribute it names, carried mapping by carried mapping, each
    /// mapping's flags in declaration order.
    owned: Vec<bool>,
}

impl AttributeOwners {
    /// A record for an entity carrying `mappings`, in which no declaration owns its attribute yet.
    #[must_use]
    pub fn new(mappings: &[Arc<Mapping>]) -> AttributeOwners {
        let mut starts = SmallVec::with_capacity(mappings.len() + 1);
        let mut total = 0;
        starts.push(total);
        for mapping in mappings {
            total += mapping.attributes().len();
            starts.push(total);
        }
        AttributeOwners {
            starts,
            owned: vec![false; total],
        }
    }

    /// Records that the declaration at `at` owns the attribute it names.
    pub fn claim(&mut self, at: CarriedDeclaration) {
        if let Some(owned) = self.position(at).and_then(|position| self.owned.get_mut(position)) {
            *owned = true;
        }
    }

    /// Whether the declaration at `at` owns the attribute it names.
    #[must_use]
    pub fn owns(&self, at: CarriedDeclaration) -> bool {
        self.position(at).and_then(|position| self.owned.get(position)).is_some_and(|owned| *owned)
    }

    /// The flag of the declaration at `at`, or `None` when the record carries no such declaration.
    fn position(&self, at: CarriedDeclaration) -> Option<usize> {
        let start = *self.starts.get(at.mapping)?;
        let end = *self.starts.get(at.mapping + 1)?;
        let position = start + at.declaration;
        (position < end).then_some(position)
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_owners::{AttributeOwners, CarriedDeclaration};
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use pretty_assertions::assert_eq;
    use std::{path::Path, sync::Arc};

    /// A mapping declaring one attribute per name in `names`.
    fn mapping(names: &[&str]) -> Arc<Mapping> {
        let attributes: Vec<String> = names.iter().map(|name| format!(r#"{name}: {{ source: "{{{{ {name} }}}}" }}"#)).collect();
        let document = format!(
            r#"{{ version: "v4", dataModel: "Sensor", identity: {{ entityName: "S-{{{{ id }}}}" }}, attributes: {{ {} }} }}"#,
            attributes.join(", ")
        );
        let mut runner = TemplateRunner::new();
        Arc::new(Mapping::from_json5(&document, Path::new("test.json5"), &mut runner).unwrap())
    }

    #[test]
    fn a_new_record_has_no_owner_for_any_declaration() {
        let owners = AttributeOwners::new(&[mapping(&["label", "count"]), mapping(&["label"])]);

        let owned: Vec<bool> = [(0, 0), (0, 1), (1, 0)]
            .into_iter()
            .map(|(mapping, declaration)| owners.owns(CarriedDeclaration { mapping, declaration }))
            .collect();
        assert_eq!(owned, [false, false, false]);
    }

    #[test]
    fn a_claimed_declaration_owns_its_attribute_and_no_other_declaration_does() {
        let mut owners = AttributeOwners::new(&[mapping(&["label", "count"]), mapping(&["label", "status"])]);

        owners.claim(CarriedDeclaration { mapping: 1, declaration: 0 });

        let owned: Vec<bool> = [(0, 0), (0, 1), (1, 0), (1, 1)]
            .into_iter()
            .map(|(mapping, declaration)| owners.owns(CarriedDeclaration { mapping, declaration }))
            .collect();
        assert_eq!(owned, [false, false, true, false]);
    }

    #[test]
    fn a_declaration_past_its_mapping_s_attributes_is_neither_claimed_nor_owned() {
        let mut owners = AttributeOwners::new(&[mapping(&["label"]), mapping(&["label"])]);

        owners.claim(CarriedDeclaration { mapping: 0, declaration: 1 });

        assert!(!owners.owns(CarriedDeclaration { mapping: 0, declaration: 1 }));
        assert!(!owners.owns(CarriedDeclaration { mapping: 1, declaration: 0 }));
        assert!(!owners.owns(CarriedDeclaration { mapping: 2, declaration: 0 }));
    }
}
