pub mod attribute_editor;
pub mod attribute_list;
pub mod data_model_name;
pub mod field;
pub mod identity_form;
pub mod model_select;
pub mod save_preview;

use crate::{
    components::popup::Popup,
    layout::centered_rect,
    screen::wizard::{state::WizardState, step::WizardStep},
    theme::Theme,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// Draws the whole wizard: header, the active screen, footer, and any popup or notification overlay.
pub fn ui(frame: &mut Frame, state: &mut WizardState, theme: &Theme) {
    frame.render_widget(Block::default().bg(theme.bg_main), frame.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(3)])
        .split(frame.area());

    render_header(frame, state, chunks[0], theme);

    match state.step {
        WizardStep::ModelSelection => model_select::render(frame, state, chunks[1], theme),
        WizardStep::DataModelNameInput => data_model_name::render(frame, state, chunks[1], theme),
        WizardStep::IdentityForm => identity_form::render(frame, state, chunks[1], theme),
        WizardStep::AttributeList => attribute_list::render(frame, state, chunks[1], theme),
        WizardStep::AttributeEditor => attribute_editor::render(frame, state, chunks[1], theme),
        WizardStep::SavePreview => save_preview::render(frame, state, chunks[1], theme),
    }

    render_footer(frame, state, chunks[2], theme);

    if let Some(popup) = &state.popup {
        Popup::new(&popup.title, &popup.message)
            .with_suggestion(&popup.suggestion)
            .with_footer("[Enter] Force Use     [Esc] Edit")
            .render(frame, frame.area(), theme);
    }

    render_notification(frame, state, theme);
}

/// Draws the fading notification banner, counting down its remaining frames.
fn render_notification(frame: &mut Frame, state: &mut WizardState, theme: &Theme) {
    let Some(notification) = &mut state.notification else {
        return;
    };

    if notification.ttl_frames == 0 {
        state.notification = None;
        return;
    }

    notification.ttl_frames -= 1;
    let area = centered_rect(60, 10, frame.area());
    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(notification.color))
        .bg(theme.bg_main);
    frame.render_widget(Paragraph::new(notification.message.clone()).block(block).alignment(Alignment::Center), area);
}

fn render_header(frame: &mut Frame, state: &WizardState, area: Rect, theme: &Theme) {
    let title = match state.step {
        WizardStep::ModelSelection => "Model Selection".to_string(),
        WizardStep::DataModelNameInput => "New Data Model".to_string(),
        WizardStep::IdentityForm => "Identity Configuration".to_string(),
        WizardStep::AttributeList => {
            if state.current_path.is_empty() {
                state.target_model.as_ref().map(ToString::to_string).unwrap_or_default()
            } else {
                state.current_path.segments().iter().map(ToString::to_string).collect::<Vec<_>>().join(" > ")
            }
        }
        WizardStep::AttributeEditor => "Attribute Editor".to_string(),
        WizardStep::SavePreview => "Save Mapping".to_string(),
    };

    let header_block = Block::default()
        .borders(Borders::ALL)
        .title(" Cassiopeia Wizard ")
        .border_style(Style::default().fg(theme.accent))
        .bg(theme.bg_header);

    frame.render_widget(
        Paragraph::new(title)
            .block(header_block)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.text_header).add_modifier(Modifier::BOLD)),
        area,
    );
}

fn render_footer(frame: &mut Frame, state: &WizardState, area: Rect, theme: &Theme) {
    let help_spans = match state.step {
        WizardStep::ModelSelection => vec![
            Span::styled("[↑↓]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Navigate  "),
            Span::styled("[Enter]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Select  "),
            Span::styled("[Esc]", Style::default().fg(theme.error)),
            Span::raw(" Quit"),
        ],
        WizardStep::DataModelNameInput => vec![
            Span::styled("[Enter]", Style::default().fg(theme.success)),
            Span::raw(" Next  "),
            Span::styled("[Esc]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Back  "),
        ],
        WizardStep::IdentityForm => vec![
            Span::styled("[Enter]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Next Step  "),
            Span::styled("[Esc]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Back  "),
        ],
        WizardStep::AttributeList => attribute_list_footer(state, theme),
        WizardStep::AttributeEditor => vec![
            Span::styled("[↑↓]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Change Field  "),
            Span::styled("[Enter]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Confirm  "),
            Span::styled("[Esc]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Cancel  "),
        ],
        WizardStep::SavePreview => vec![
            Span::styled("[Enter]", Style::default().fg(theme.success)),
            Span::raw(" Save  "),
            Span::styled("[Esc]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Cancel  "),
        ],
    };

    let footer_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border_secondary))
        .bg(theme.bg_footer);

    frame.render_widget(Paragraph::new(Line::from(help_spans)).block(footer_block).alignment(Alignment::Center), area);
}

/// Builds the attribute-list footer, which gains a Back hint once the user has descended a level.
fn attribute_list_footer<'a>(state: &WizardState, theme: &Theme) -> Vec<Span<'a>> {
    let mut spans = vec![
        Span::styled("[A]", Style::default().fg(theme.border_secondary)),
        Span::raw("dd  "),
        Span::styled("[D]", Style::default().fg(theme.border_secondary)),
        Span::raw("elete  "),
        Span::styled("[E]", Style::default().fg(theme.border_secondary)),
        Span::raw("dit  "),
        Span::styled("[S]", Style::default().fg(theme.border_secondary)),
        Span::raw("ave"),
    ];

    if !state.current_path.is_empty() {
        spans.extend([
            Span::raw("  "),
            Span::styled("[Esc]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Back"),
        ]);
    }

    spans.extend([
        Span::raw("  "),
        Span::styled("[Enter]", Style::default().fg(theme.border_secondary)),
        Span::raw(" Navigate  "),
        Span::styled("[Esc]", Style::default().fg(theme.error)),
        Span::raw(" Quit"),
    ]);

    spans
}
