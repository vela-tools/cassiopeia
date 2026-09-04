pub mod conversion;

use crate::screen::wizard::editor_key::EditorKey;
use cassiopeia_mapping::transformation::Transformation;
use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
use indexmap::IndexMap;
use serde_json::Value;

/// One attribute as the wizard edits it, before it becomes a mapping document.
///
/// This is deliberately not `cassiopeia_mapping::attribute::Attribute`. A mapping's attributes are
/// keyed by validated NGSI-LD names, but the wizard's editing tree carries synthetic keys for
/// `oneOf` choices such as `Option 1 - GeoJSON Point`; those keys are not valid attribute names.
/// The editing tree therefore keys nodes by [`EditorKey`] and only turns into the domain
/// type, with its names and target models validated, when the mapping is saved (see [`conversion`]).
#[derive(Debug, Clone)]
pub struct EditorAttribute {
    /// The NGSI-LD attribute type this will produce.
    pub attribute_type: NgsiLdAttributeKind,

    /// The value conversion, when one is chosen.
    pub transformation: Option<Transformation>,

    /// The source template, or list of templates, the user has entered. Absent until mapped.
    pub source: Option<Value>,

    /// Nested attribute declarations, keyed by their editing-tree key.
    pub mappings: IndexMap<EditorKey, EditorAttribute>,

    /// Per-language declarations preserved across edits.
    pub language_map: IndexMap<EditorKey, EditorAttribute>,

    /// The raw target model string entered for a relationship, validated only at save time.
    pub target_entity: Option<String>,
}

impl EditorAttribute {
    /// Builds a leaf attribute of the given type and conversion, with no source and no children.
    #[must_use]
    pub fn new(attribute_type: NgsiLdAttributeKind, transformation: Option<Transformation>) -> EditorAttribute {
        EditorAttribute {
            attribute_type,
            transformation,
            source: None,
            mappings: IndexMap::new(),
            language_map: IndexMap::new(),
            target_entity: None,
        }
    }

    /// Whether this attribute is a structured-object container, that is, whether it holds nested
    /// declarations rather than reading a value of its own.
    #[must_use]
    pub fn is_container(&self) -> bool {
        self.transformation == Some(Transformation::Object)
    }
}

#[cfg(test)]
mod tests {
    use crate::editor::EditorAttribute;
    use cassiopeia_mapping::transformation::Transformation;
    use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;

    #[test]
    fn a_new_attribute_has_no_source_or_children() {
        let attribute = EditorAttribute::new(NgsiLdAttributeKind::Property, Some(Transformation::String));
        assert!(attribute.source.is_none());
        assert!(attribute.mappings.is_empty());
        assert!(!attribute.is_container());
    }

    #[test]
    fn an_object_transformation_marks_a_container() {
        let attribute = EditorAttribute::new(NgsiLdAttributeKind::Property, Some(Transformation::Object));
        assert!(attribute.is_container());
    }
}
