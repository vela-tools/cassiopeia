use crate::{editor::EditorAttribute, screen::wizard::editor_key::EditorKey};
use cassiopeia_mapping::{
    conversion::{
        Conversion,
        declaration_shape::{DeclarationShape, ValueOrigin},
    },
    transformation::Transformation,
};
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
        ("Iri", Transformation::Iri),
        ("Urn", Transformation::Urn),
        ("Array", Transformation::Array),
        ("Object", Transformation::Object),
        ("Json", Transformation::Json),
        ("Geometry", Transformation::Geometry),
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

    /// What the attribute being edited is built from: the object its nested `mappings` assemble in
    /// container mode, its `source` otherwise.
    const fn shape(&self) -> DeclarationShape {
        if self.is_container_mode {
            DeclarationShape::Value(ValueOrigin::Assembled)
        } else {
            DeclarationShape::Value(ValueOrigin::Sourced)
        }
    }

    /// Whether the conversion at `transform_idx` (offset by one, so `0` is "no conversion") loads
    /// under the selected type and the attribute's shape.
    fn admits(&self, transform_idx: usize) -> bool {
        let kind = Self::TYPES[self.attr_type_idx];
        let transformation = transform_idx
            .checked_sub(1)
            .and_then(|index| Self::TRANSFORMS.get(index))
            .map(|(_, transform)| *transform);

        Conversion::resolve(kind, transformation, self.shape()).is_ok()
    }

    /// The next conversion in `direction` that the selected type admits, wrapping through "no
    /// conversion", so the editor never offers a combination the mapping would refuse to load.
    #[must_use]
    pub fn next_admitted_transformation(&self, direction: CycleDirection) -> usize {
        let count = Self::TRANSFORMS.len() + 1;
        let mut candidate = direction.step(self.transform_idx, count);
        for _ in 0..count {
            if candidate == 0 || self.admits(candidate) {
                return candidate;
            }
            candidate = direction.step(candidate, count);
        }

        0
    }

    /// Clears the selected conversion when the selected type does not admit it, as after the type
    /// changes.
    pub fn drop_inadmissible_transformation(&mut self) {
        if !self.admits(self.transform_idx) {
            self.transform_idx = 0;
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
        screen::wizard::{
            attribute_editor::{AttributeEditorState, CycleDirection},
            editor_key::EditorKey,
        },
    };
    use cassiopeia_mapping::transformation::Transformation;
    use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
    use pretty_assertions::assert_eq;
    use serde_json::Value;

    #[test]
    fn from_editor_splits_an_array_source_into_committed_tags() {
        let mut attribute = EditorAttribute::new(NgsiLdAttributeKind::Property, Some(Transformation::String));
        attribute.source = Some(Value::Array(vec![Value::String("{{ a }}".to_string()), Value::String("{{ b }}".to_string())]));

        let editor = AttributeEditorState::from_editor("field".to_string(), &attribute, Some(EditorKey::SchemaField("field".to_string())));

        assert_eq!(editor.source_parts, vec!["{{ a }}", "{{ b }}"]);
        assert_eq!(editor.source_input, "");
    }

    /// The labels the editor offers, in order, cycling forward from "no conversion" until it returns
    /// to it, for an attribute of `kind`.
    fn offered_labels(kind: NgsiLdAttributeKind) -> Vec<String> {
        let mut editor = AttributeEditorState::new();
        editor.attr_type_idx = AttributeEditorState::TYPES.iter().position(|candidate| *candidate == kind).unwrap();
        let mut labels = Vec::new();
        loop {
            editor.transform_idx = editor.next_admitted_transformation(CycleDirection::Next);
            if editor.transform_idx == 0 {
                return labels;
            }
            labels.push(editor.transform_label());
        }
    }

    #[test]
    fn the_editor_offers_only_the_transformations_the_selected_type_admits() {
        assert_eq!(offered_labels(NgsiLdAttributeKind::JsonProperty), ["Array", "Object", "Json"]);
        assert_eq!(offered_labels(NgsiLdAttributeKind::Relationship), Vec::<String>::new());
        assert_eq!(offered_labels(NgsiLdAttributeKind::VocabProperty), ["Iri", "Urn"]);
        let every_transformation = AttributeEditorState::TRANSFORMS
            .iter()
            .map(|(label, _)| (*label).to_owned())
            .collect::<Vec<_>>();
        assert_eq!(offered_labels(NgsiLdAttributeKind::Property), every_transformation);
        assert_eq!(offered_labels(NgsiLdAttributeKind::ListProperty), every_transformation);
    }

    #[test]
    fn cycling_backwards_offers_the_same_transformations_in_reverse() {
        let mut editor = AttributeEditorState::new();
        editor.attr_type_idx = AttributeEditorState::TYPES
            .iter()
            .position(|kind| *kind == NgsiLdAttributeKind::JsonProperty)
            .unwrap();

        editor.transform_idx = editor.next_admitted_transformation(CycleDirection::Previous);

        assert_eq!(editor.transform_label(), "Json");
    }

    #[test]
    fn changing_to_a_type_that_does_not_admit_the_conversion_clears_it() {
        let mut editor = AttributeEditorState::new();
        editor.transform_idx = AttributeEditorState::TRANSFORMS
            .iter()
            .position(|(_, transform)| *transform == Transformation::Integer)
            .unwrap()
            + 1;
        editor.attr_type_idx = AttributeEditorState::TYPES
            .iter()
            .position(|kind| *kind == NgsiLdAttributeKind::Relationship)
            .unwrap();

        editor.drop_inadmissible_transformation();

        assert_eq!(editor.transform_idx, 0);
    }

    #[test]
    fn changing_to_a_type_that_admits_the_conversion_keeps_it() {
        let mut editor = AttributeEditorState::new();
        let object = AttributeEditorState::TRANSFORMS
            .iter()
            .position(|(_, transform)| *transform == Transformation::Object)
            .unwrap()
            + 1;
        editor.transform_idx = object;
        editor.attr_type_idx = AttributeEditorState::TYPES
            .iter()
            .position(|kind| *kind == NgsiLdAttributeKind::JsonProperty)
            .unwrap();

        editor.drop_inadmissible_transformation();

        assert_eq!(editor.transform_idx, object);
    }

    #[test]
    fn a_blank_editor_selects_the_first_type_and_no_conversion() {
        let editor = AttributeEditorState::new();
        assert_eq!(editor.attr_type_idx, 0);
        assert_eq!(editor.transform_idx, 0);
        assert_eq!(editor.transform_label(), "None (Default)");
    }
}
