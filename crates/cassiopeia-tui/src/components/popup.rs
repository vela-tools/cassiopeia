use crate::{layout::centered_rect, theme::Theme};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

/// A modal message box drawn centered over the screen, with an optional suggestion line and footer.
/// Used to surface a validation warning that the user can accept or dismiss.
pub struct Popup<'a> {
    title: &'a str,
    message: &'a str,
    suggestion: Option<&'a str>,
    footer: Option<&'a str>,
}

impl<'a> Popup<'a> {
    /// Builds a popup titled `title` showing `message`.
    #[must_use]
    pub const fn new(title: &'a str, message: &'a str) -> Popup<'a> {
        Popup {
            title,
            message,
            suggestion: None,
            footer: None,
        }
    }

    /// Adds a suggestion line beneath the message.
    #[must_use]
    pub const fn with_suggestion(mut self, suggestion: &'a str) -> Popup<'a> {
        self.suggestion = Some(suggestion);
        self
    }

    /// Adds a footer line at the bottom of the box.
    #[must_use]
    pub const fn with_footer(mut self, footer: &'a str) -> Popup<'a> {
        self.footer = Some(footer);
        self
    }

    /// Draws the popup over `area`.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let popup_area = centered_rect(60, 35, area);

        frame.render_widget(Clear, popup_area);

        let popup_block = Block::default()
            .borders(Borders::ALL)
            .title(self.title)
            .border_style(Style::default().fg(theme.accent))
            .bg(theme.bg_main);

        frame.render_widget(&popup_block, popup_area);

        let inner = popup_block.inner(popup_area);

        let mut constraints = vec![Constraint::Length(3)];
        if self.suggestion.is_some() {
            constraints.push(Constraint::Length(2));
            constraints.push(Constraint::Length(1));
        }
        if self.footer.is_some() {
            constraints.push(Constraint::Min(1));
            constraints.push(Constraint::Length(1));
        }

        let chunks = Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);

        frame.render_widget(
            Paragraph::new(self.message)
                .style(Style::default().fg(theme.error))
                .alignment(Alignment::Center),
            chunks[0],
        );

        if let Some(suggestion) = self.suggestion {
            frame.render_widget(
                Paragraph::new(format!("Suggestion: {suggestion}"))
                    .style(Style::default().fg(theme.key_optional).add_modifier(Modifier::BOLD))
                    .alignment(Alignment::Center),
                chunks[2],
            );
        }

        if let Some(footer) = self.footer {
            frame.render_widget(
                Paragraph::new(footer).style(Style::default().fg(theme.text_dim)).alignment(Alignment::Center),
                chunks[chunks.len() - 1],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::components::popup::Popup;

    #[test]
    fn the_builder_records_the_optional_lines() {
        let popup = Popup::new("Title", "Body").with_suggestion("fix").with_footer("[Enter]");
        assert_eq!(popup.suggestion, Some("fix"));
        assert_eq!(popup.footer, Some("[Enter]"));
    }

    #[test]
    fn a_bare_popup_has_no_optional_lines() {
        let popup = Popup::new("Title", "Body");
        assert!(popup.suggestion.is_none());
        assert!(popup.footer.is_none());
    }
}
