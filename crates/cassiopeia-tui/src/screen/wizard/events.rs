use crate::{
    notification::Notification,
    screen::wizard::{
        attribute_editor::{AttributeEditorState, CycleDirection},
        popup::{PopupAction, WizardPopup},
        state::WizardState,
        step::WizardStep,
    },
    theme::Theme,
};
use cassiopeia_ngsi_ld::{data_model::DataModel, entity::attribute::NgsiLdAttributeKind};
use crossterm::event::{KeyCode, KeyEvent};
use inflections::case::{is_camel_case, is_pascal_case, to_camel_case, to_pascal_case};
use serde_json::Value;
use std::str::FromStr;

/// Something the wizard must react to: a keystroke carrying the active theme, or a schema loading.
pub enum WizardAction {
    /// A keystroke, paired with the active theme so notification colors can be chosen.
    KeyPress(KeyEvent, Theme),
    /// A dereferenced schema finished loading and should populate the editing tree.
    SchemaLoaded(Value),
}

/// Applies actions to the wizard state. A namespace for the state transition, holding no state.
pub struct WizardReducer;

impl WizardReducer {
    /// Advances `state` in response to `action`.
    pub fn apply(state: &mut WizardState, action: WizardAction) {
        match action {
            WizardAction::KeyPress(key, theme) => Self::on_key(state, key, theme),
            WizardAction::SchemaLoaded(json) => state.load_schema(&json),
        }
    }

    /// Routes a keystroke to the popup handler, or to the handler for the active wizard screen.
    fn on_key(state: &mut WizardState, key: KeyEvent, theme: Theme) {
        if state.popup.is_some() {
            Self::on_popup_key(state, key, theme);
            return;
        }

        match state.step {
            WizardStep::ModelSelection => Self::on_model_selection_key(state, key),
            WizardStep::DataModelNameInput => Self::on_data_model_name_key(state, key, theme),
            WizardStep::IdentityForm => Self::on_identity_form_key(state, key),
            WizardStep::AttributeList => Self::on_attribute_list_key(state, key, theme),
            WizardStep::AttributeEditor => Self::on_attribute_editor_key(state, key),
            WizardStep::SavePreview => Self::on_save_preview_key(state, key),
        }
    }

    /// Handles a keystroke while a validation-warning popup is open.
    fn on_popup_key(state: &mut WizardState, key: KeyEvent, theme: Theme) {
        let Some(popup) = &state.popup else { return };

        match key.code {
            KeyCode::Enter => {
                match popup.action {
                    PopupAction::ForceModelName => Self::confirm_model_name(state, theme),
                    PopupAction::ForceAttributeName => state.save_attribute(),
                }
                state.popup = None;
            }
            KeyCode::Esc => state.popup = None,
            // A popup only reacts to accept and dismiss.
            KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Char(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => {}
        }
    }

    /// Parses the typed new-model name into a data model and advances, or notifies on failure.
    fn confirm_model_name(state: &mut WizardState, theme: Theme) {
        match DataModel::from_str(state.new_model_input.trim()) {
            Ok(model) => {
                state.target_model = Some(model);
                state.step = WizardStep::IdentityForm;
            }
            Err(error) => state.notification = Some(Notification::new(format!("Error: {error}"), theme.error, 60)),
        }
    }

    /// Handles a keystroke on the model-selection screen.
    fn on_model_selection_key(state: &mut WizardState, key: KeyEvent) {
        match key.code {
            KeyCode::Char(character) => {
                state.model_picker.search_query.push(character);
                state.model_picker.update_search();
            }
            KeyCode::Backspace => {
                state.model_picker.search_query.pop();
                state.model_picker.update_search();
            }
            KeyCode::Down => state.model_picker.move_down(),
            KeyCode::Up => state.model_picker.move_up(),
            KeyCode::Esc => state.should_quit = true,
            // Model selection is otherwise driven by Enter, handled by the launcher.
            KeyCode::Enter
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => {}
        }
    }

    /// Handles a keystroke on the new-model-name screen, validating `PascalCase` on confirm.
    fn on_data_model_name_key(state: &mut WizardState, key: KeyEvent, theme: Theme) {
        match key.code {
            KeyCode::Char(character) => state.new_model_input.push(character),
            KeyCode::Backspace => {
                state.new_model_input.pop();
            }
            KeyCode::Enter => {
                let input = state.new_model_input.trim().to_string();
                if input.is_empty() {
                    return;
                }

                if is_pascal_case(&input) {
                    match DataModel::from_str(&input) {
                        Ok(model) => {
                            state.target_model = Some(model);
                            state.step = WizardStep::IdentityForm;
                        }
                        Err(error) => state.notification = Some(Notification::new(format!("Error: {error}"), theme.error, 60)),
                    }
                } else {
                    state.popup = Some(WizardPopup {
                        title: "Validation Warning".to_string(),
                        message: format!("The input '{input}' is not PascalCase."),
                        suggestion: to_pascal_case(&input),
                        action: PopupAction::ForceModelName,
                    });
                }
            }
            KeyCode::Esc => state.step = WizardStep::ModelSelection,
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => {}
        }
    }

    /// Handles a keystroke on the identity-configuration form.
    fn on_identity_form_key(state: &mut WizardState, key: KeyEvent) {
        match key.code {
            KeyCode::Char(character) => state.identity_form.entity_id_template.push(character),
            KeyCode::Backspace => {
                state.identity_form.entity_id_template.pop();
            }
            KeyCode::Enter => {
                if !state.identity_form.entity_id_template.is_empty() {
                    state.step = WizardStep::AttributeList;
                }
            }
            KeyCode::Esc => state.step = WizardStep::ModelSelection,
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => {}
        }
    }

    /// Handles a keystroke on the attribute-list screen.
    fn on_attribute_list_key(state: &mut WizardState, key: KeyEvent, theme: Theme) {
        match key.code {
            KeyCode::Down => {
                let current = state.attr_list_state.selected().unwrap_or(0);
                let next = (current + 1) % state.get_current_map().len().max(1);
                state.attr_list_state.select(Some(next));
            }
            KeyCode::Up => {
                let current = state.attr_list_state.selected().unwrap_or(0);
                let previous = if current == 0 {
                    state.get_current_map().len().saturating_sub(1)
                } else {
                    current - 1
                };
                state.attr_list_state.select(Some(previous));
            }
            KeyCode::Char('a') => {
                state.attribute_editor = AttributeEditorState::new();
                state.step = WizardStep::AttributeEditor;
            }
            KeyCode::Char('d') => state.delete_selected_attribute(theme.error),
            KeyCode::Char('e') => Self::edit_selected(state, theme),
            KeyCode::Char('s') => state.step = WizardStep::SavePreview,
            KeyCode::Enter => Self::open_selected(state, theme),
            KeyCode::Esc | KeyCode::Backspace => state.go_back(),
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Char(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => {}
        }
    }

    /// Edits the selected attribute unless it is a locked `oneOf` option.
    fn edit_selected(state: &mut WizardState, theme: Theme) {
        let Some(index) = state.attr_list_state.selected() else { return };
        let keys = state.get_current_sorted_keys();
        let Some(key) = keys.get(index) else { return };

        if state.is_attribute_disabled(key) {
            state.notification = Some(Notification::new("Cannot edit disabled oneOf option".to_string(), theme.error, 30));
        } else {
            state.start_edit_attribute();
        }
    }

    /// Opens the selected attribute: descends into a container, or edits a leaf.
    fn open_selected(state: &mut WizardState, theme: Theme) {
        let Some(index) = state.attr_list_state.selected() else { return };
        let keys = state.get_current_sorted_keys();
        let Some(key) = keys.get(index) else { return };
        let Some(attribute) = state.get_current_map().get(key) else { return };

        if attribute.is_container() {
            state.enter_folder();
        } else if state.is_attribute_disabled(key) {
            state.notification = Some(Notification::new("Cannot edit disabled oneOf option".to_string(), theme.error, 30));
        } else {
            state.start_edit_attribute();
        }
    }

    /// Handles a keystroke on the save-preview screen.
    fn on_save_preview_key(state: &mut WizardState, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            state.step = WizardStep::AttributeList;
        }
    }

    /// Handles a keystroke in the attribute editor: field navigation, value cycling, and text entry.
    fn on_attribute_editor_key(state: &mut WizardState, key: KeyEvent) {
        let editor = &mut state.attribute_editor;
        let idx = editor.focus_index;
        let is_relationship = matches!(
            AttributeEditorState::TYPES[editor.attr_type_idx],
            NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship
        );
        let field_count = if is_relationship { 5 } else { 4 };

        match key.code {
            KeyCode::Tab | KeyCode::Down => editor.focus_index = (idx + 1) % field_count,
            KeyCode::BackTab | KeyCode::Up => editor.focus_index = if idx == 0 { field_count - 1 } else { idx - 1 },
            KeyCode::Left => Self::cycle_field(editor, idx, CycleDirection::Previous),
            KeyCode::Right => Self::cycle_field(editor, idx, CycleDirection::Next),
            KeyCode::Char(character) => Self::type_into_field(editor, idx, character),
            KeyCode::Backspace => Self::backspace_field(editor, idx),
            KeyCode::Enter => Self::confirm_editor(state),
            KeyCode::Esc => state.step = WizardStep::AttributeList,
            KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => {}
        }
    }

    /// Cycles the type or transformation field left or right, respecting schema-derived locks.
    fn cycle_field(editor: &mut AttributeEditorState, idx: usize, direction: CycleDirection) {
        if idx == 1 && !editor.is_schema_derived {
            editor.attr_type_idx = direction.step(editor.attr_type_idx, AttributeEditorState::TYPES.len());
        }

        if idx == 2 && Self::transform_is_cycleable(editor) {
            if Self::transform_is_numeric_locked(editor) {
                editor.transform_idx = editor.get_next_number_transformation(direction);
            } else {
                editor.transform_idx = direction.step(editor.transform_idx, AttributeEditorState::TRANSFORMS.len() + 1);
            }
        }
    }

    /// Whether the transformation field responds to cycling in the editor's current mode.
    const fn transform_is_cycleable(editor: &AttributeEditorState) -> bool {
        !editor.is_container_mode || Self::transform_is_numeric_locked(editor)
    }

    /// Whether the transformation is locked to the integer/float pair by a schema-derived property.
    const fn transform_is_numeric_locked(editor: &AttributeEditorState) -> bool {
        editor.is_schema_derived && matches!(AttributeEditorState::TYPES[editor.attr_type_idx], NgsiLdAttributeKind::Property)
    }

    /// Appends a character to the focused text field, when that field accepts typing.
    fn type_into_field(editor: &mut AttributeEditorState, idx: usize, character: char) {
        match idx {
            0 if !editor.is_schema_derived => editor.name.push(character),
            3 if !editor.is_container_mode => editor.source_input.push(character),
            4 => editor.target_entity.push(character),
            _ => {}
        }
    }

    /// Removes a character from the focused text field, popping a source tag when the input is empty.
    fn backspace_field(editor: &mut AttributeEditorState, idx: usize) {
        match idx {
            0 if !editor.is_schema_derived => {
                editor.name.pop();
            }
            3 if !editor.is_container_mode => {
                if editor.source_input.is_empty() && !editor.source_parts.is_empty() {
                    editor.source_parts.pop();
                } else {
                    editor.source_input.pop();
                }
            }
            4 => {
                editor.target_entity.pop();
            }
            _ => {}
        }
    }

    /// Confirms the attribute editor: commits a source tag, or saves the attribute after validating
    /// its name is camelCase.
    fn confirm_editor(state: &mut WizardState) {
        let editor = &mut state.attribute_editor;

        if editor.focus_index == 3 && !editor.source_input.trim().is_empty() {
            let tag = editor.source_input.trim().to_string();
            editor.source_parts.push(tag);
            editor.source_input.clear();
            return;
        }

        if editor.is_schema_derived {
            state.save_attribute();
            return;
        }

        let input = editor.name.trim().to_string();
        if input.is_empty() {
            return;
        }

        if is_camel_case(&input) {
            state.save_attribute();
        } else {
            state.popup = Some(WizardPopup {
                title: "Validation Warning".to_string(),
                message: format!("The input '{input}' is not camelCase."),
                suggestion: to_camel_case(&input),
                action: PopupAction::ForceAttributeName,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        screen::wizard::{
            events::{WizardAction, WizardReducer},
            state::WizardState,
            step::WizardStep,
        },
        theme::DARK,
    };
    use cassiopeia_ngsi_ld::data_model::DataModel;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::str::FromStr;

    fn press(state: &mut WizardState, code: KeyCode) {
        WizardReducer::apply(state, WizardAction::KeyPress(KeyEvent::new(code, KeyModifiers::NONE), DARK));
    }

    #[test]
    fn typing_on_model_selection_filters_the_picker() {
        let mut state = WizardState::new(vec![DataModel::from_str("Sensor").unwrap(), DataModel::from_str("Device").unwrap()]);
        press(&mut state, KeyCode::Char('s'));
        assert!(state.model_picker.filtered.iter().any(|choice| choice.to_string() == "Sensor"));
        assert!(!state.model_picker.filtered.iter().any(|choice| choice.to_string() == "Device"));
    }

    #[test]
    fn confirming_a_valid_new_model_name_advances_to_the_identity_form() {
        let mut state = WizardState::new(Vec::new());
        state.step = WizardStep::DataModelNameInput;
        state.new_model_input = "Sensor".to_string();
        press(&mut state, KeyCode::Enter);
        assert_eq!(state.step, WizardStep::IdentityForm);
        assert_eq!(state.target_model.as_ref().map(ToString::to_string), Some("Sensor".to_string()));
    }

    #[test]
    fn a_non_pascalcase_model_name_opens_a_validation_popup() {
        let mut state = WizardState::new(Vec::new());
        state.step = WizardStep::DataModelNameInput;
        state.new_model_input = "sensor".to_string();
        press(&mut state, KeyCode::Enter);
        assert!(state.popup.is_some());
        assert_eq!(state.step, WizardStep::DataModelNameInput);
    }
}
