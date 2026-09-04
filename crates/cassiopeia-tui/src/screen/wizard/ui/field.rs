use crate::theme::Theme;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// One labelled field in a wizard form, drawn focused, unfocused, or locked.
///
/// A struct rather than a free function so the several display inputs travel together instead of as
/// a long argument list.
pub struct Field<'a> {
    pub title: &'a str,
    pub content: &'a str,
    pub field_idx: usize,
    pub focus_idx: usize,
    pub is_disabled: bool,
}

impl Field<'_> {
    /// Draws the field into `area`, styling it by whether it is focused or locked.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let is_focused = self.focus_idx == self.field_idx;
        let title = if is_focused {
            format!("> {} ", self.title)
        } else {
            format!("  {} ", self.title)
        };

        let (title_style, content_style) = if self.is_disabled {
            (
                Style::default().fg(theme.locked).add_modifier(Modifier::DIM),
                Style::default().fg(theme.locked).add_modifier(Modifier::DIM),
            )
        } else if is_focused {
            (
                Style::default().fg(theme.input_active).add_modifier(Modifier::BOLD),
                Style::default().fg(theme.text_primary),
            )
        } else {
            (Style::default().fg(theme.border_secondary), Style::default().fg(theme.text_dim))
        };

        frame.render_widget(
            Paragraph::new(self.content)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .title_style(title_style)
                        .border_style(title_style)
                        .bg(theme.bg_surface),
                )
                .style(content_style),
            area,
        );
    }
}
