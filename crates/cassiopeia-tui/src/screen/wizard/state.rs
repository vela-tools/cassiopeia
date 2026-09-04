use crate::{
    editor::{
        EditorAttribute,
        conversion::{ConversionError, filter_mapped, to_domain_attributes},
    },
    model_picker::ModelPicker,
    notification::Notification,
    schema_node_details::SchemaNodeDetails,
    schema_parser::parse_schema,
    screen::wizard::{
        attribute_editor::AttributeEditorState,
        editor_key::EditorKey,
        identity_state::IdentityState,
        model_choice::ModelChoice,
        node_path::NodePath,
        popup::WizardPopup,
        step::WizardStep,
    },
};
use cassiopeia_mapping::{
    identity::Identity,
    mapping::Mapping,
    template::{TemplateSource, runner::TemplateRunner},
    transformation::Transformation,
    version::Version,
};
use cassiopeia_ngsi_ld::{data_model::DataModel, entity::attribute::NgsiLdAttributeKind};
use indexmap::IndexMap;
use ratatui::{style::Color, widgets::ListState};
use serde_json::Value;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

/// Everything the mapping wizard keeps between frames: the model picker, the attribute tree being
/// edited, the sub-form states, and any transient popup or notification.
pub struct WizardState {
    /// Which wizard screen is showing.
    pub step: WizardStep,
    /// Set when the user asks to quit the wizard.
    pub should_quit: bool,

    /// The searchable list of model choices, including the create-new action.
    pub model_picker: ModelPicker<ModelChoice>,

    /// The model the mapping targets, set once a model is chosen or a new name is confirmed.
    pub target_model: Option<DataModel>,
    /// The attribute tree being edited, keyed by editing-tree key.
    pub root_attributes: IndexMap<EditorKey, EditorAttribute>,
    /// The path from the tree root to the map currently on screen.
    pub current_path: NodePath,
    /// The attribute names the schema marks required at the top level.
    pub required_fields: HashSet<String>,
    /// The per-node schema detail facts, keyed by path from the root.
    pub schema_details: HashMap<NodePath, SchemaNodeDetails>,

    /// The text entered on the new-model-name screen.
    pub new_model_input: String,
    /// The identity form's editing state.
    pub identity_form: IdentityState,
    /// The attribute editor's editing state.
    pub attribute_editor: AttributeEditorState,
    /// Which attribute-list row is highlighted.
    pub attr_list_state: ListState,

    /// The open validation-warning popup, if any.
    pub popup: Option<WizardPopup>,
    /// The transient notification banner, if any.
    pub notification: Option<Notification>,

    /// The filename the finished mapping is saved under.
    pub save_filename: String,
}

impl WizardState {
    /// Starts the wizard on the model-selection screen, offering `models` plus a create-new action.
    #[must_use]
    pub fn new(models: Vec<DataModel>) -> WizardState {
        let mut choices = vec![ModelChoice::CreateNew];
        choices.extend(models.into_iter().map(ModelChoice::Existing));
        let mut model_picker = ModelPicker::new(choices);
        model_picker.select_first();

        let mut attr_list_state = ListState::default();
        attr_list_state.select(Some(0));

        WizardState {
            step: WizardStep::ModelSelection,
            should_quit: false,
            model_picker,
            target_model: None,
            root_attributes: IndexMap::new(),
            current_path: NodePath::new(),
            required_fields: HashSet::new(),
            schema_details: HashMap::new(),
            new_model_input: String::new(),
            identity_form: IdentityState::default(),
            attribute_editor: AttributeEditorState::new(),
            attr_list_state,
            popup: None,
            notification: None,
            save_filename: String::new(),
        }
    }

    /// Loads a dereferenced schema into the wizard's editing tree.
    pub fn load_schema(&mut self, json: &Value) {
        let parsed = parse_schema(json);
        self.root_attributes = parsed.attributes;
        self.required_fields = parsed.required;
        self.schema_details = parsed.details;
    }

    /// The attribute map at the current navigation depth.
    #[must_use]
    pub fn get_current_map(&self) -> &IndexMap<EditorKey, EditorAttribute> {
        let mut current = &self.root_attributes;
        for segment in self.current_path.segments() {
            match current.get(segment) {
                Some(attribute) => current = &attribute.mappings,
                None => break,
            }
        }
        current
    }

    /// Runs `action` against the attribute map at the current navigation depth, mutably.
    ///
    /// A closure rather than a returned `&mut` keeps each borrow local while the map is descended by
    /// a runtime path.
    pub fn with_current_map_mut<R>(&mut self, action: impl FnOnce(&mut IndexMap<EditorKey, EditorAttribute>) -> R) -> R {
        fn descend<R>(
            map: &mut IndexMap<EditorKey, EditorAttribute>,
            path: &[EditorKey],
            action: impl FnOnce(&mut IndexMap<EditorKey, EditorAttribute>) -> R,
        ) -> R {
            match path.split_first() {
                None => action(map),
                Some((key, rest)) => match map.get_mut(key) {
                    Some(attribute) => descend(&mut attribute.mappings, rest, action),
                    // The navigation invariant keeps the path valid; a stray segment stops here.
                    None => action(map),
                },
            }
        }

        let path = self.current_path.clone();
        descend(&mut self.root_attributes, path.segments(), action)
    }

    /// The current map's keys, sorted required-first, then containers, then by display text.
    #[must_use]
    pub fn get_current_sorted_keys(&self) -> Vec<EditorKey> {
        let map = self.get_current_map();
        let mut keys: Vec<EditorKey> = map.keys().cloned().collect();
        keys.sort_by(|first, second| {
            let required_first = self.current_path.is_empty() && self.required_fields.contains(first.to_string().as_str());
            let required_second = self.current_path.is_empty() && self.required_fields.contains(second.to_string().as_str());
            let folder_first = map.get(first).is_some_and(EditorAttribute::is_container);
            let folder_second = map.get(second).is_some_and(EditorAttribute::is_container);

            match (required_first, required_second) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (true, true) | (false, false) => match (folder_first, folder_second) {
                    (true, false) => Ordering::Less,
                    (false, true) => Ordering::Greater,
                    (true, true) | (false, false) => first.to_string().cmp(&second.to_string()),
                },
            }
        });
        keys
    }

    /// Whether a `oneOf` option is locked because a sibling option is already mapped.
    #[must_use]
    pub fn is_attribute_disabled(&self, key: &EditorKey) -> bool {
        key.is_one_of_option() && self.check_oneof_sibling_mapped(key)
    }

    /// Whether any sibling `oneOf` option of `key` already carries a source.
    #[must_use]
    pub fn check_oneof_sibling_mapped(&self, key: &EditorKey) -> bool {
        if !key.is_one_of_option() {
            return false;
        }

        self.get_current_map()
            .iter()
            .any(|(sibling_key, sibling)| sibling_key.is_one_of_option() && sibling_key != key && sibling.source.is_some())
    }

    /// Descends into the selected container attribute.
    pub fn enter_folder(&mut self) {
        if let Some(index) = self.attr_list_state.selected() {
            let keys = self.get_current_sorted_keys();
            if let Some(key) = keys.get(index)
                && self.get_current_map().get(key).is_some_and(EditorAttribute::is_container)
            {
                self.current_path.push(key.clone());
                self.attr_list_state.select(Some(0));
            }
        }
    }

    /// Steps back out of a container, or back to the identity form at the top level.
    pub fn go_back(&mut self) {
        if self.current_path.is_empty() {
            self.step = WizardStep::IdentityForm;
        } else {
            self.current_path.pop();
            self.attr_list_state.select(Some(0));
        }
    }

    /// Opens the attribute editor pre-filled from the selected attribute.
    pub fn start_edit_attribute(&mut self) {
        let Some(index) = self.attr_list_state.selected() else { return };
        let keys = self.get_current_sorted_keys();
        let Some(key) = keys.get(index) else { return };
        let map = self.get_current_map();
        let Some(attribute) = map.get(key) else { return };

        let transform_idx = match attribute.transformation {
            Some(transform) => AttributeEditorState::TRANSFORMS
                .iter()
                .position(|(_, value)| *value == transform)
                .map_or(0, |position| position + 1),
            None => 0,
        };
        let is_container = attribute.is_container();
        let is_schema_derived = !key.is_one_of_option();

        let mut editor = AttributeEditorState::from_editor(key.to_string(), attribute, Some(key.clone()));
        editor.transform_idx = transform_idx;
        editor.focus_index = 0;
        editor.is_container_mode = is_container;
        editor.is_schema_derived = is_schema_derived;

        self.attribute_editor = editor;
        self.step = WizardStep::AttributeEditor;
    }

    /// Writes the attribute editor's contents back into the tree, then returns to the list.
    pub fn save_attribute(&mut self) {
        let state = self.attribute_editor.clone();
        if state.name.trim().is_empty() {
            return;
        }

        let new_name = state.name.clone();
        let old_key = state.original_key.clone();

        // A `oneOf` option keeps its typed identity only while its name is untouched; renaming it, or
        // any new attribute, becomes a plain schema field.
        let new_key = match &old_key {
            Some(key) if key.is_one_of_option() && new_name == key.to_string() => key.clone(),
            Some(_) | None => EditorKey::SchemaField(new_name.clone()),
        };
        let is_oneof_option = new_key.is_one_of_option();

        let mut preserved_mappings = IndexMap::new();
        if let Some(old_key) = &old_key {
            let removed = self.with_current_map_mut(|map| map.shift_remove(old_key));
            if let Some(removed) = removed {
                preserved_mappings = removed.mappings;
            }
        }

        if let Some(old_key) = &old_key
            && *old_key != new_key
            && self.current_path.is_empty()
            && self.required_fields.remove(old_key.to_string().as_str())
        {
            self.required_fields.insert(new_name);
        }

        let combined_source = if state.source_input.trim().is_empty() {
            state.source_parts.clone()
        } else {
            let mut combined = state.source_parts.clone();
            combined.push(state.source_input.trim().to_string());
            combined
        };

        if is_oneof_option && !combined_source.is_empty() {
            self.clear_sibling_options(&new_key);
        }

        let attr_type = AttributeEditorState::TYPES[state.attr_type_idx];
        let transformation = if state.is_container_mode {
            Some(Transformation::Object)
        } else if state.transform_idx == 0 {
            None
        } else {
            Some(AttributeEditorState::TRANSFORMS[state.transform_idx - 1].1)
        };
        let target_entity = matches!(attr_type, NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship).then(|| state.target_entity.clone());
        let source = match combined_source.as_slice() {
            [] => None,
            [single] => Some(Value::String(single.clone())),
            many => Some(Value::Array(many.iter().cloned().map(Value::String).collect())),
        };

        let attribute = EditorAttribute {
            attribute_type: attr_type,
            transformation,
            source,
            mappings: preserved_mappings,
            language_map: IndexMap::new(),
            target_entity,
        };

        self.with_current_map_mut(|map| map.insert(new_key, attribute));
        self.step = WizardStep::AttributeList;
    }

    /// Clears the source of every sibling `oneOf` option, so only the just-saved one stays mapped.
    fn clear_sibling_options(&mut self, saved_key: &EditorKey) {
        let sibling_keys: Vec<EditorKey> = self
            .get_current_map()
            .keys()
            .filter(|key| key.is_one_of_option() && *key != saved_key)
            .cloned()
            .collect();

        self.with_current_map_mut(|map| {
            for sibling_key in sibling_keys {
                let cleared = map.get(&sibling_key).map(|sibling| {
                    let mut cleared = sibling.clone();
                    cleared.source = None;
                    cleared
                });
                if let Some(cleared) = cleared {
                    map.insert(sibling_key, cleared);
                }
            }
        });
    }

    /// Deletes the selected attribute, refusing to remove a schema-required top-level field.
    pub fn delete_selected_attribute(&mut self, theme_error: Color) {
        let Some(index) = self.attr_list_state.selected() else { return };
        let keys = self.get_current_sorted_keys();
        let Some(key_to_delete) = keys.get(index) else { return };

        if self.current_path.is_empty() && self.required_fields.contains(key_to_delete.to_string().as_str()) {
            self.notification = Some(Notification::new(
                "Error: Cannot delete a REQUIRED field defined by the Data Model.".to_string(),
                theme_error,
                30,
            ));
            return;
        }

        let is_empty = self.with_current_map_mut(|map| {
            map.shift_remove(key_to_delete);
            map.is_empty()
        });

        if index > 0 {
            self.attr_list_state.select(Some(index - 1));
        } else if is_empty {
            self.attr_list_state.select(None);
        } else {
            self.attr_list_state.select(Some(0));
        }
    }

    /// Builds the mapping document from the current editing state against `data_model`.
    ///
    /// Fallible because every attribute name entered so far is only validated here, at the boundary
    /// where the editing tree becomes a mapping document. The target model is already typed, so it
    /// no longer needs validating.
    ///
    /// # Errors
    /// Returns a [`ConversionError`] when an attribute name or a relationship target model in the
    /// editing tree is not valid.
    pub fn build_mapping(&self, data_model: &DataModel, runner: &mut TemplateRunner) -> Result<Mapping, ConversionError> {
        let attributes = to_domain_attributes(&filter_mapped(&self.root_attributes))?;
        let identity = Identity::new(TemplateSource::new(self.identity_form.entity_id_template.clone()), None);

        Ok(Mapping::new(Version::V4, data_model.clone(), identity, attributes, runner))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        editor::EditorAttribute,
        screen::wizard::{attribute_editor::AttributeEditorState, editor_key::EditorKey, state::WizardState},
    };
    use cassiopeia_mapping::{template::runner::TemplateRunner, transformation::Transformation};
    use cassiopeia_ngsi_ld::{data_model::DataModel, entity::attribute::NgsiLdAttributeKind};
    use serde_json::Value;
    use std::str::FromStr;

    fn field(name: &str) -> EditorKey {
        EditorKey::SchemaField(name.to_string())
    }

    fn option(index: u32, title: &str) -> EditorKey {
        EditorKey::OneOfOption {
            index,
            title: title.to_string(),
        }
    }

    fn mapped_leaf() -> EditorAttribute {
        let mut attribute = EditorAttribute::new(NgsiLdAttributeKind::Property, Some(Transformation::String));
        attribute.source = Some(Value::String("{{ t }}".to_string()));
        attribute
    }

    fn display_keys(state: &WizardState) -> Vec<String> {
        state.get_current_sorted_keys().iter().map(ToString::to_string).collect()
    }

    #[test]
    fn sorted_keys_put_required_then_folders_then_alphabetical() {
        let mut state = WizardState::new(Vec::new());
        state
            .root_attributes
            .insert(field("zebra"), EditorAttribute::new(NgsiLdAttributeKind::Property, None));
        state
            .root_attributes
            .insert(field("alpha"), EditorAttribute::new(NgsiLdAttributeKind::Property, None));
        state.root_attributes.insert(
            field("folder"),
            EditorAttribute::new(NgsiLdAttributeKind::Property, Some(Transformation::Object)),
        );
        state
            .root_attributes
            .insert(field("temperature"), EditorAttribute::new(NgsiLdAttributeKind::Property, None));
        state.required_fields.insert("temperature".to_string());

        assert_eq!(display_keys(&state), vec!["temperature", "folder", "alpha", "zebra"]);
    }

    #[test]
    fn sorted_option_keys_tie_break_on_the_display_string() {
        let mut state = WizardState::new(Vec::new());
        state
            .root_attributes
            .insert(option(2, "Alpha"), EditorAttribute::new(NgsiLdAttributeKind::Property, None));
        state
            .root_attributes
            .insert(option(1, "Zulu"), EditorAttribute::new(NgsiLdAttributeKind::Property, None));

        assert_eq!(display_keys(&state), vec!["Option 1 - Zulu", "Option 2 - Alpha"]);
    }

    #[test]
    fn an_unmapped_option_is_disabled_once_a_sibling_option_is_mapped() {
        let mut state = WizardState::new(Vec::new());
        state.root_attributes.insert(option(1, "Point"), mapped_leaf());
        state
            .root_attributes
            .insert(option(2, "Address"), EditorAttribute::new(NgsiLdAttributeKind::Property, None));

        assert!(state.is_attribute_disabled(&option(2, "Address")));
        assert!(!state.is_attribute_disabled(&option(1, "Point")));
        assert!(!state.is_attribute_disabled(&field("plain")));
    }

    #[test]
    fn saving_a_mapped_option_clears_its_siblings_sources() {
        let mut state = WizardState::new(Vec::new());
        let mut first = mapped_leaf();
        first.source = Some(Value::String("{{ a }}".to_string()));
        state.root_attributes.insert(option(1, "Point"), first);
        state.root_attributes.insert(option(2, "Address"), mapped_leaf());

        let key = option(2, "Address");
        let mut editor = AttributeEditorState::from_editor(key.to_string(), &mapped_leaf(), Some(key));
        editor.source_parts = vec!["{{ b }}".to_string()];
        state.attribute_editor = editor;
        state.save_attribute();

        assert!(state.root_attributes.get(&option(1, "Point")).unwrap().source.is_none());
        assert!(state.root_attributes.get(&option(2, "Address")).unwrap().source.is_some());
    }

    #[test]
    fn build_mapping_serializes_a_simple_mapping_to_the_expected_document() {
        let mut state = WizardState::new(Vec::new());
        state.identity_form.entity_id_template = "Sensor-{{ id }}".to_string();
        state.root_attributes.insert(field("temperature"), mapped_leaf());

        let model = DataModel::from_str("Sensor").unwrap();
        let mut runner = TemplateRunner::new();
        let mapping = state.build_mapping(&model, &mut runner).unwrap();
        let json = serde_json::to_string(&mapping).unwrap();

        assert_eq!(
            json,
            r#"{"version":"v4","dataModel":"Sensor","identity":{"entityName":"Sensor-{{ id }}"},"attributes":{"temperature":{"type":"Property","transformation":"string","source":"{{ t }}"}}}"#
        );
    }
}
