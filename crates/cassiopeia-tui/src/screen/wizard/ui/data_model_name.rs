use crate::{components::input::TextInput, layout::centered_rect, screen::wizard::state::WizardState, theme::Theme};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// Draws the new-model-name form: a prompt and a single text field for the model name.
pub fn render(frame: &mut Frame, state: &mut WizardState, area: Rect, theme: &Theme) {
    let form_area = centered_rect(50, 40, area);

    let form_block = Block::default()
        .borders(Borders::ALL)
        .title(" New Data Model ")
        .border_style(Style::default().fg(theme.border_secondary))
        .bg(theme.bg_surface);

    frame.render_widget(&form_block, form_area);

    let inner_area = form_block.inner(form_area);
    let chunks = Layout::default()
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Length(3), Constraint::Min(1)])
        .split(inner_area);

    frame.render_widget(
        Paragraph::new("Enter a name for your new data model (e.g., SmartDevice)")
            .style(Style::default().fg(theme.text_dim))
            .alignment(Alignment::Center),
        chunks[0],
    );

    TextInput::new("Model Name", &state.new_model_input, true).render(frame, chunks[1], theme);
}
