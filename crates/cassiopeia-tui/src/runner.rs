use crate::{config::TuiConfig, error::Result};
use crossterm::{
    cursor,
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::{
    io::{self, Stdout},
    panic,
};

/// Owns the terminal lifecycle for one screen: it enters the alternate screen, runs a draw/event
/// loop, and restores the terminal afterwards even if the loop panics.
pub struct TuiRunner {
    config: TuiConfig,
}

impl TuiRunner {
    /// Wraps the launch configuration a screen will draw with.
    #[must_use]
    pub const fn new(config: TuiConfig) -> TuiRunner {
        TuiRunner { config }
    }

    /// Puts the terminal into raw mode on the alternate screen, ready for full-screen drawing.
    ///
    /// # Errors
    /// Returns [`TuiError::Io`](crate::error::TuiError::Io) when the terminal cannot be reconfigured.
    pub fn setup() -> Result<Terminal<CrosstermBackend<Stdout>>> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, cursor::Hide)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        Ok(terminal)
    }

    /// Returns the terminal to its original mode and screen.
    ///
    /// # Errors
    /// Returns [`TuiError::Io`](crate::error::TuiError::Io) when the terminal cannot be restored.
    pub fn restore() -> Result<()> {
        terminal::disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, cursor::Show)?;

        Ok(())
    }

    /// Runs `screen` against a freshly set-up terminal, restoring the terminal before returning.
    ///
    /// A panic hook restores the terminal first, so a panic inside the draw loop cannot leave the
    /// user's terminal in raw mode.
    ///
    /// # Errors
    /// Returns a [`TuiError`](crate::error::TuiError) when terminal setup, the draw loop, or restore
    /// fails.
    pub fn run<F>(&self, mut screen: F) -> Result<()>
    where
        F: FnMut(&mut Terminal<CrosstermBackend<Stdout>>, &TuiConfig) -> Result<()>,
    {
        let mut terminal = Self::setup()?;

        let original_hook = panic::take_hook();
        panic::set_hook(Box::new(move |panic_info| {
            let _ = Self::restore();
            original_hook(panic_info);
        }));

        let result = screen(&mut terminal, &self.config);

        Self::restore()?;

        result
    }
}
