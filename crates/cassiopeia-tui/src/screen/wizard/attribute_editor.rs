use crate::{editor::EditorAttribute, screen::wizard::editor_key::EditorKey};
use cassiopeia_mapping::transformation::Transformation;
use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
use serde_json::Value;

/// The direction the editor cycles a selectable field, driven by the left/right arrows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleDirection {
    /// Move to the previous option, wrapping to the last.
    Previous,
    /// Move to the next option, wrapping to the first.
    Next,
}

impl CycleDirection {
    /// The index reached by stepping `index` one place in this direction within `count` options.
    #[must_use]
    pub const fn step(self, index: usize, count: usize) -> usize {
        match self {
            CycleDirection::Next => (index + 1) % count,
            CycleDirection::Previous => (index + count - 1) % count,
        }
    }
}

/// The editing state of the attribute editor screen: the field values, which field holds focus, and
/// the flags that lock fields whose shape is fixed by the schema.
#[derive(Debug, Clone, Default)]
pub struct AttributeEditorState {
    /// The attribute name being typed. Pre-validation text, validated into a name only at save.
    pub name: String,
    /// The index into [`AttributeEditorState::TYPES`] of the selected attribute type.
    pub attr_type_idx: usize,
    /// The committed source templates, each shown as a tag.
    pub source_parts: Vec<String>,
    /// The source template currently being typed, before it is committed as a tag.
    pub source_input: String,
    /// The index into [`AttributeEditorState::TRANSFORMS`] of the selected conversion, offset by one
    /// so index `0` means "no conversion".
    pub transform_idx: usize,
    /// The relationship target model being typed. Pre-validation text, validated at save.
    pub target_entity: String,
    /// Which editor field holds focus.
    pub focus_index: usize,
    /// The tree key of the attribute being edited, or `None` for a brand-new attribute.
    pub original_key: Option<EditorKey>,
    /// Whether the attribute is a structured-object container, locking its conversion to `Object`.
    pub is_container_mode: bool,
    /// Whether the attribute came from the schema, locking its name and type.
    pub is_schema_derived: bool,
}

impl AttributeEditorState {
    /// The attribute types the editor cycles through, in display order.
    pub const TYPES: &'static [NgsiLdAttributeKind] = &[
        NgsiLdAttributeKind::Property,
        NgsiLdAttributeKind::Relationship,
        NgsiLdAttributeKind::GeoProperty,
        NgsiLdAttributeKind::ListRelationship,
        NgsiLdAttributeKind::LanguageProperty,
        NgsiLdAttributeKind::VocabProperty,
        NgsiLdAttributeKind::ListProperty,
        NgsiLdAttributeKind::JsonProperty,
    ];

    /// The value conversions the editor cycles through, each paired with its display label.
    pub const TRANSFORMS: &'static [(&'static str, Transformation)] = &[
        ("Boolean", Transformation::Boolean),
        ("Integer", Transformation::Integer),
        ("Float", Transformation::Float),
        ("String", Transformation::String),
        ("Array", Transformation::Array),
        ("Object", Transformation::Object),
        ("Point", Transformation::Point),
        ("MultiPoint", Transformation::MultiPoint),
        ("LineString", Transformation::LineString),
        ("MultiLineString", Transformation::MultiLineString),
        ("Polygon", Transformation::Polygon),
        ("MultiPolygon", Transformation::MultiPolygon),
        ("DateTime", Transformation::DateTime),
        ("Date", Transformation::Date),
        ("Time", Transformation::Time),
    ];

    /// A blank editor for a brand-new attribute.
    #[must_use]
    pub fn new() -> AttributeEditorState {
        AttributeEditorState::default()
    }

    /// An editor pre-filled from an existing attribute, ready to edit it in place.
    #[must_use]
    pub fn from_editor(name: String, attribute: &EditorAttribute, original_key: Option<EditorKey>) -> AttributeEditorState {
        let (source_parts, source_input) = match &attribute.source {
            None => (Vec::new(), String::new()),
            Some(Value::String(text)) if text.is_empty() => (Vec::new(), String::new()),
            Some(Value::String(text)) => (vec![text.clone()], String::new()),
            Some(Value::Array(items)) => (items.iter().filter_map(Value::as_str).map(str::to_string).collect(), String::new()),
            Some(other) => (Vec::new(), other.to_string()),
        };

        AttributeEditorState {
            name,
            attr_type_idx: Self::TYPES.iter().position(|kind| *kind == attribute.attribute_type).unwrap_or(0),
            source_parts,
            source_input,
            transform_idx: 0,
            target_entity: attribute.target_entity.clone().unwrap_or_default(),
            focus_index: 0,
            original_key,
            is_container_mode: false,
            is_schema_derived: false,
        }
    }

    /// The label for the currently selected attribute type.
    #[must_use]
    pub fn type_label(&self) -> String {
        format!("{:?}", Self::TYPES[self.attr_type_idx])
    }

    /// The label for the currently selected value conversion.
    #[must_use]
    pub fn transform_label(&self) -> String {
        if self.is_container_mode {
            return "Object (Locked)".to_string();
        }

        if self.transform_idx == 0 {
            "None (Default)".to_string()
        } else {
            Self::TRANSFORMS[self.transform_idx - 1].0.to_string()
        }
    }

    /// The next numeric conversion when the transform field is locked to numeric options.
    ///
    /// A schema-derived numeric property may only be an integer or a float; this cycles between
    /// those two in `direction` without leaving the numeric pair.
    #[must_use]
    pub fn get_next_number_transformation(&self, direction: CycleDirection) -> usize {
        let number_transforms = [Transformation::Integer, Transformation::Float];
        let current_transform = self
            .transform_idx
            .checked_sub(1)
            .and_then(|index| Self::TRANSFORMS.get(index))
            .map(|(_, transform)| *transform);
        let current_idx = current_transform
            .and_then(|transform| number_transforms.iter().position(|number| *number == transform))
            .unwrap_or(0);

        let next_idx = direction.step(current_idx, number_transforms.len());

        Self::TRANSFORMS
            .iter()
            .position(|(_, value)| *value == number_transforms[next_idx])
            .map_or(0, |index| index + 1)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        editor::EditorAttribute,
        screen::wizard::{attribute_editor::AttributeEditorState, editor_key::EditorKey},
    };
    use cassiopeia_mapping::transformation::Transformation;
    use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
    use serde_json::Value;

    #[test]
    fn from_editor_splits_an_array_source_into_committed_tags() {
        let mut attribute = EditorAttribute::new(NgsiLdAttributeKind::Property, Some(Transformation::String));
        attribute.source = Some(Value::Array(vec![Value::String("{{ a }}".to_string()), Value::String("{{ b }}".to_string())]));

        let editor = AttributeEditorState::from_editor("field".to_string(), &attribute, Some(EditorKey::SchemaField("field".to_string())));

        assert_eq!(editor.source_parts, vec!["{{ a }}", "{{ b }}"]);
        assert!(editor.source_input.is_empty());
    }

    #[test]
    fn a_blank_editor_selects_the_first_type_and_no_conversion() {
        let editor = AttributeEditorState::new();
        assert_eq!(editor.attr_type_idx, 0);
        assert_eq!(editor.transform_idx, 0);
        assert_eq!(editor.transform_label(), "None (Default)");
    }
}
