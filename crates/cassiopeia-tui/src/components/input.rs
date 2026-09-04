use crate::theme::Theme;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// A single-line labelled text field. Purely presentational: it renders the `value` it is handed and
/// highlights itself when `is_active`, leaving keystroke handling to the screen that owns the value.
pub struct TextInput<'a> {
    title: &'a str,
    value: &'a str,
    is_active: bool,
}

impl<'a> TextInput<'a> {
    /// Builds a field titled `title` showing `value`, drawn as active when `is_active`.
    #[must_use]
    pub const fn new(title: &'a str, value: &'a str, is_active: bool) -> TextInput<'a> {
        TextInput { title, value, is_active }
    }

    /// Draws the field into `area`.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .border_style(if self.is_active {
                Style::default().fg(theme.input_active).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.border_secondary)
            })
            .bg(theme.bg_surface);

        frame.render_widget(
            Paragraph::new(self.value).block(block).style(if self.is_active {
                Style::default().fg(theme.text_primary)
            } else {
                Style::default().fg(theme.text_dim)
            }),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::components::input::TextInput;

    #[test]
    fn a_field_records_its_title_value_and_active_state() {
        let input = TextInput::new("Search", "abc", true);
        assert_eq!(input.title, "Search");
        assert_eq!(input.value, "abc");
        assert!(input.is_active);
    }
}
