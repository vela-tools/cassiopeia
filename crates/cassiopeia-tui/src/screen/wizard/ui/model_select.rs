use crate::{components::input::TextInput, screen::wizard::state::WizardState, theme::Theme};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem},
};

/// Draws the model-selection screen: a search box over a list of models, the "create new" entry
/// highlighted in the success color.
pub fn render(frame: &mut Frame, state: &mut WizardState, area: Rect, theme: &Theme) {
    let chunks = Layout::default().constraints([Constraint::Length(3), Constraint::Min(1)]).split(area);

    TextInput::new("Search", &state.model_picker.search_query, true).render(frame, chunks[0], theme);

    let items: Vec<ListItem> = state
        .model_picker
        .filtered
        .iter()
        .map(|model| {
            ListItem::new(format!(" {model} ")).style(if model.is_create_new() {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.text_dim)
            })
        })
        .collect();

    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(" Available Models ")
        .border_style(Style::default().fg(theme.accent));

    frame.render_stateful_widget(
        List::new(items)
            .block(list_block)
            .highlight_style(Style::default().bg(theme.input_active).fg(Color::Black).add_modifier(Modifier::BOLD)),
        chunks[1],
        &mut state.model_picker.list_state,
    );
}
