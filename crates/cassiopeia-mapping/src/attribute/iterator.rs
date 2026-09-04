use crate::attribute::Attribute;

/// A depth-first walk over an attribute and everything nested beneath it.
///
/// Nested object mappings, language maps, and attribute-level properties all hold further
/// attributes, so anything that inspects a mapping as a whole has to descend through all three.
pub struct AttributeIterator<'a> {
    stack: Vec<&'a Attribute>,
}

impl<'a> AttributeIterator<'a> {
    /// Starts a walk at one attribute.
    pub(crate) fn new(root: &'a Attribute) -> AttributeIterator<'a> {
        AttributeIterator { stack: vec![root] }
    }
}

impl<'a> Iterator for AttributeIterator<'a> {
    type Item = &'a Attribute;

    fn next(&mut self) -> Option<&'a Attribute> {
        let attribute = self.stack.pop()?;

        // Pushed in reverse so the stack yields each group in declaration order, and pushed
        // last-group-first so properties are visited before language maps before nested mappings.
        self.stack.extend(attribute.mappings().values().rev());
        self.stack.extend(attribute.language_map().values().rev());
        if let Some(properties) = attribute.properties() {
            self.stack.extend(properties.values().rev());
        }

        Some(attribute)
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::Attribute;

    #[test]
    fn the_walk_descends_through_mappings_language_maps_and_properties() {
        let attribute: Attribute = serde_json5::from_str(
            r#"{
                "mappings": {"a": {"source": "{{ a }}"}},
                "languageMap": {"en": {"source": "{{ en }}"}},
                "properties": {"observedAt": {"source": "{{ t }}"}}
            }"#,
        )
        .unwrap();

        assert_eq!(attribute.iter_recursive().count(), 4);
    }

    #[test]
    fn a_leaf_attribute_yields_only_itself() {
        let attribute: Attribute = serde_json5::from_str(r#"{"source": "{{ a }}"}"#).unwrap();

        assert_eq!(attribute.iter_recursive().count(), 1);
    }
}
