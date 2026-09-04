use crate::{components::input::TextInput, layout::centered_rect, screen::wizard::state::WizardState, theme::Theme};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// Draws the identity form: a prompt and a text field for the entity-id template.
pub fn render(frame: &mut Frame, state: &mut WizardState, area: Rect, theme: &Theme) {
    let form_area = centered_rect(50, 50, area);

    let form_block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " Identity Configuration - {} ",
            state.target_model.as_ref().map(ToString::to_string).unwrap_or_default()
        ))
        .border_style(Style::default().fg(theme.border_secondary))
        .bg(theme.bg_surface);

    frame.render_widget(&form_block, form_area);

    let inner_area = form_block.inner(form_area);
    let chunks = Layout::default()
        .constraints([Constraint::Length(3), Constraint::Length(1), Constraint::Length(3), Constraint::Min(1)])
        .split(inner_area);

    frame.render_widget(
        Paragraph::new("Configure the identity components for entity generation")
            .style(Style::default().fg(theme.text_dim))
            .alignment(Alignment::Center),
        chunks[0],
    );

    TextInput::new("Entity ID Template", &state.identity_form.entity_id_template, true).render(frame, chunks[2], theme);
}
