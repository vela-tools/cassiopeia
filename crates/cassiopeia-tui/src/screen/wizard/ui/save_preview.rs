use crate::{layout::centered_rect, screen::wizard::state::WizardState, theme::Theme};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// Draws the save-preview screen: a confirmation message and the output filename.
pub fn render(frame: &mut Frame, state: &mut WizardState, area: Rect, theme: &Theme) {
    let summary_area = centered_rect(50, 40, area);

    let chunks = Layout::default()
        .constraints([Constraint::Length(5), Constraint::Length(3)])
        .split(summary_area);

    let summary_block = Block::default()
        .borders(Borders::ALL)
        .title(" Save Preview ")
        .border_style(Style::default().fg(theme.accent))
        .bg(theme.bg_main);

    frame.render_widget(
        Paragraph::new("Ready to save mapping configuration.\nPress [Enter] to write file.\nPress [Esc] to cancel.")
            .block(summary_block)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.text_primary)),
        chunks[0],
    );

    let filename_block = Block::default()
        .borders(Borders::ALL)
        .title(" Output File ")
        .border_style(Style::default().fg(theme.input_active))
        .bg(theme.bg_main);

    frame.render_widget(
        Paragraph::new(format!(" > {}", state.save_filename))
            .block(filename_block)
            .style(Style::default().fg(theme.text_primary)),
        chunks[1],
    );
}
