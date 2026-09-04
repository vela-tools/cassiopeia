use cassiopeia_tui::theme::{DARK, LIGHT, Theme};

/// Which terminal palette a TUI screen should launch with.
///
/// A named choice rather than a bare boolean, so a call site reads as the palette it selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    Dark,
    Light,
}

impl ThemeChoice {
    /// Selects the palette a `--light` presence flag chooses.
    ///
    /// A set flag launches the light palette; an unset flag launches the dark one.
    #[must_use]
    pub const fn from_light(light: bool) -> ThemeChoice {
        if light { ThemeChoice::Light } else { ThemeChoice::Dark }
    }

    /// The palette this choice selects.
    #[must_use]
    pub const fn theme(self) -> Theme {
        match self {
            ThemeChoice::Dark => DARK,
            ThemeChoice::Light => LIGHT,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::theme_choice::ThemeChoice;
    use cassiopeia_tui::theme::{DARK, LIGHT};

    #[test]
    fn each_choice_selects_its_own_palette() {
        assert_eq!(ThemeChoice::Dark.theme().bg_main, DARK.bg_main);
        assert_eq!(ThemeChoice::Light.theme().bg_main, LIGHT.bg_main);
    }

    #[test]
    fn the_two_palettes_are_distinct() {
        assert_ne!(ThemeChoice::Dark.theme().bg_main, ThemeChoice::Light.theme().bg_main);
    }

    #[test]
    fn a_set_light_flag_selects_the_light_palette() {
        assert_eq!(ThemeChoice::from_light(true), ThemeChoice::Light);
    }

    #[test]
    fn an_unset_light_flag_selects_the_dark_palette() {
        assert_eq!(ThemeChoice::from_light(false), ThemeChoice::Dark);
    }
}
