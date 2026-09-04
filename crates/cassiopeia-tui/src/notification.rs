use ratatui::style::Color;

/// A transient banner shown over a screen, fading out after a fixed number of frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The message text to display.
    pub message: String,
    /// The color the banner is drawn in.
    pub color: Color,
    /// How many more frames the banner stays visible before it disappears.
    pub ttl_frames: u32,
}

impl Notification {
    /// Builds a notification shown for `ttl_frames` frames.
    #[must_use]
    pub const fn new(message: String, color: Color, ttl_frames: u32) -> Notification {
        Notification { message, color, ttl_frames }
    }
}

#[cfg(test)]
mod tests {
    use crate::notification::Notification;
    use ratatui::style::Color;

    #[test]
    fn a_notification_holds_its_message_color_and_lifetime() {
        let notification = Notification::new("saved".to_string(), Color::Green, 30);
        assert_eq!(notification.message, "saved");
        assert_eq!(notification.color, Color::Green);
        assert_eq!(notification.ttl_frames, 30);
    }
}
