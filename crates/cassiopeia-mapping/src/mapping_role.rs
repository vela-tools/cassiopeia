/// Where a mapping is declared, which decides how the records it maps relate to the entities it
/// emits.
///
/// The role is fixed by the document's own structure rather than by anything an author writes: a
/// mapping bound to an input is a [`Document`](MappingRole::Document), and a `syntheticEntity` lifted
/// out of an attribute is [`Synthetic`](MappingRole::Synthetic).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum MappingRole {
    /// A mapping document bound to an input, whose `identity` names the entity each record
    /// describes. Two records it maps to one id are two descriptions of the same entity.
    #[default]
    Document,
    /// A `syntheticEntity` declared on an attribute, emitting the entity a record names, such as the
    /// country a mountain lies in. Many records naming the same target is the purpose of the
    /// declaration, so every record that names it contributes to the one entity by design.
    Synthetic,
}

#[cfg(test)]
mod tests {
    use crate::{mapping::Mapping, mapping_role::MappingRole, template::runner::TemplateRunner};
    use std::path::Path;

    #[test]
    fn a_mapping_document_and_the_synthetic_entity_it_declares_carry_their_own_roles() {
        let document = r#"{
            version: "v4",
            dataModel: "Mountain",
            identity: { entityName: "{{ name }}" },
            attributes: {
                hasCountry: {
                    source: "{{ country }}",
                    type: "Relationship",
                    target: { entity: "Country" },
                    syntheticEntity: {
                        dataModel: "Country",
                        identity: { entityName: "{{ country }}" },
                        attributes: {},
                    },
                },
            },
        }"#;
        let mapping = Mapping::from_json5(document, Path::new("test.json5"), &mut TemplateRunner::new()).unwrap();
        let synthetic = mapping
            .attributes()
            .values()
            .find_map(|attribute| attribute.synthetic_entity().as_ref())
            .unwrap();

        assert_eq!(mapping.role(), MappingRole::Document);
        assert_eq!(synthetic.role(), MappingRole::Synthetic);
    }
}
