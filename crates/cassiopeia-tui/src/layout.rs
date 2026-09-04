use ratatui::prelude::*;

/// Builds a rectangle centered inside `area`, taking up `percent_x` of its width and `percent_y` of
/// its height. Used to place modal forms and popups over the full-screen background.
#[must_use]
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use crate::layout::centered_rect;
    use ratatui::layout::Rect;

    #[test]
    fn a_centered_rect_is_smaller_than_and_inside_its_area() {
        let area = Rect::new(0, 0, 100, 100);
        let centered = centered_rect(50, 40, area);
        assert!(centered.width <= area.width && centered.height <= area.height);
        assert!(centered.x >= area.x && centered.y >= area.y);
    }
}
