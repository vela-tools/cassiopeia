use crate::{
    screen::explorer::state::{ExplorerScreen, ExplorerState},
    theme::Theme,
};
use cassiopeia_ngsi_ld::data_model::DataModel;
use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

/// Something the explorer must react to: a keystroke, or a schema finishing loading.
pub enum ExplorerAction {
    /// A keystroke on the active screen.
    KeyPress(KeyEvent),
    /// A dereferenced schema finished loading for the given model.
    SchemaLoaded(DataModel, Value, Theme),
}

/// Applies actions to the explorer state. Holds no state of its own; it is a namespace for the
/// state transition so the event loop and the transition live in different files.
pub struct ExplorerReducer;

impl ExplorerReducer {
    /// Advances `state` in response to `action`.
    pub fn apply(state: &mut ExplorerState, action: ExplorerAction) {
        match action {
            ExplorerAction::KeyPress(key) => Self::on_key(state, key),
            ExplorerAction::SchemaLoaded(model, json, theme) => {
                if let Err(error) = state.load_schema(&model, &json, &theme) {
                    state.error_message = Some(error.to_string());
                }
            }
        }
    }

    /// Handles a keystroke on whichever screen is showing.
    fn on_key(state: &mut ExplorerState, key: KeyEvent) {
        if state.error_message.is_some() {
            state.error_message = None;
            return;
        }

        match state.current_screen {
            ExplorerScreen::ModelSelection => Self::on_selection_key(state, key),
            ExplorerScreen::SchemaViewer => Self::on_viewer_key(state, key),
        }
    }

    /// Handles a keystroke on the model-selection screen.
    fn on_selection_key(state: &mut ExplorerState, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => state.should_quit = true,
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
            // KeyCode is an external enum with dozens of variants; the explorer reacts to a handful.
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

    /// Handles a keystroke on the schema-viewer screen.
    fn on_viewer_key(state: &mut ExplorerState, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => state.current_screen = ExplorerScreen::ModelSelection,
            KeyCode::Left => {
                state.tree_state.key_left();
            }
            KeyCode::Right => {
                state.tree_state.key_right();
            }
            KeyCode::Down => {
                state.tree_state.key_down();
            }
            KeyCode::Up => {
                state.tree_state.key_up();
            }
            KeyCode::Enter => {
                state.tree_state.toggle_selected();
            }
            // KeyCode is an external enum with dozens of variants; the viewer reacts to a handful.
            KeyCode::Backspace
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
}

#[cfg(test)]
mod tests {
    use crate::screen::explorer::{
        events::{ExplorerAction, ExplorerReducer},
        state::ExplorerState,
    };
    use cassiopeia_ngsi_ld::data_model::DataModel;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::str::FromStr;

    fn press(state: &mut ExplorerState, code: KeyCode) {
        ExplorerReducer::apply(state, ExplorerAction::KeyPress(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    #[test]
    fn typing_on_the_selector_filters_the_picker() {
        let mut state = ExplorerState::new(vec![DataModel::from_str("Sensor").unwrap(), DataModel::from_str("Device").unwrap()]);
        press(&mut state, KeyCode::Char('s'));
        assert_eq!(state.model_picker.filtered.len(), 1);
        assert_eq!(state.model_picker.filtered[0].to_string(), "Sensor");
    }

    #[test]
    fn a_keystroke_dismisses_an_error_overlay() {
        let mut state = ExplorerState::new(Vec::new());
        state.error_message = Some("boom".to_string());
        press(&mut state, KeyCode::Esc);
        assert!(state.error_message.is_none());
        assert!(!state.should_quit);
    }
}
