use ratatui::style::Color;

/// The full color palette one screen renders against.
///
/// Two named instances exist, [`DARK`] and [`LIGHT`], chosen at launch; every widget reads its
/// colors from the active theme rather than hard-coding them, so a screen looks consistent and the
/// palette can be swapped in one place.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// The main window background.
    pub bg_main: Color,
    /// The background of raised surfaces such as forms and panes.
    pub bg_surface: Color,
    /// The header bar background.
    pub bg_header: Color,
    /// The footer bar background.
    pub bg_footer: Color,
    /// The primary text color.
    pub text_primary: Color,
    /// The dimmed text color for secondary information.
    pub text_dim: Color,
    /// The header text color.
    pub text_header: Color,
    /// The accent color for emphasis and borders.
    pub accent: Color,
    /// The highlight color for the active input or row.
    pub input_active: Color,
    /// The color of secondary borders.
    pub border_secondary: Color,
    /// The color marking a required attribute.
    pub key_required: Color,
    /// The color marking an optional attribute.
    pub key_optional: Color,
    /// The color of type labels.
    pub type_color: Color,
    /// The color of container/folder icons.
    pub folder_color: Color,
    /// The color for error messages.
    pub error: Color,
    /// The color for success messages.
    pub success: Color,
    /// The background color of tags.
    pub tag_bg: Color,
    /// The text color of tags.
    pub tag_text: Color,
    /// The color of locked or disabled elements.
    pub locked: Color,
}

/// The dark palette, used unless the caller asks for [`LIGHT`].
pub const DARK: Theme = Theme {
    bg_main: Color::Rgb(25, 25, 35),
    bg_surface: Color::Rgb(40, 40, 50),
    bg_header: Color::Rgb(35, 35, 45),
    bg_footer: Color::Rgb(35, 35, 45),
    text_primary: Color::Rgb(220, 220, 230),
    text_dim: Color::Rgb(140, 145, 160),
    text_header: Color::Rgb(235, 235, 245),
    accent: Color::Rgb(120, 180, 255),
    input_active: Color::Rgb(130, 190, 255),
    border_secondary: Color::Rgb(100, 110, 130),
    key_required: Color::Rgb(210, 90, 90),
    key_optional: Color::Rgb(120, 180, 255),
    type_color: Color::Rgb(180, 150, 230),
    folder_color: Color::Rgb(200, 150, 100),
    error: Color::Rgb(220, 100, 100),
    success: Color::Rgb(80, 180, 120),
    tag_bg: Color::Rgb(60, 70, 80),
    tag_text: Color::Rgb(200, 210, 220),
    locked: Color::Rgb(80, 80, 90),
};

/// The light palette, selected with the `--light` flag.
pub const LIGHT: Theme = Theme {
    bg_main: Color::Rgb(248, 249, 250),
    bg_surface: Color::Rgb(240, 241, 243),
    bg_header: Color::Rgb(235, 236, 240),
    bg_footer: Color::Rgb(235, 236, 240),
    text_primary: Color::Rgb(92, 97, 102),
    text_dim: Color::Rgb(135, 140, 150),
    text_header: Color::Rgb(60, 64, 70),
    accent: Color::Rgb(57, 158, 230),
    input_active: Color::Rgb(49, 153, 225),
    border_secondary: Color::Rgb(180, 185, 195),
    key_required: Color::Rgb(234, 108, 109),
    key_optional: Color::Rgb(57, 158, 230),
    type_color: Color::Rgb(158, 117, 199),
    folder_color: Color::Rgb(180, 130, 60),
    error: Color::Rgb(240, 113, 113),
    success: Color::Rgb(108, 191, 67),
    tag_bg: Color::Rgb(211, 225, 245),
    tag_text: Color::Rgb(60, 64, 70),
    locked: Color::Rgb(190, 195, 205),
};

impl Default for Theme {
    fn default() -> Theme {
        DARK
    }
}

#[cfg(test)]
mod tests {
    use crate::theme::{DARK, LIGHT, Theme};

    #[test]
    fn the_default_theme_is_the_dark_palette() {
        assert_eq!(Theme::default().bg_main, DARK.bg_main);
        assert_ne!(DARK.bg_main, LIGHT.bg_main);
    }
}
