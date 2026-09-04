use crate::{
    config::TuiConfig,
    error::Result,
    runner::TuiRunner,
    screen::explorer::{
        events::{ExplorerAction, ExplorerReducer},
        state::{ExplorerScreen, ExplorerState},
        ui::ui,
    },
};
use cassiopeia_ngsi_ld::data_model::DataModel;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use serde_json::Value;
use std::{fmt::Display, time::Duration};

/// Configures and launches the read-only schema explorer.
///
/// `schema_provider` resolves a data model to its dereferenced JSON Schema; it is a closure so the
/// TUI stays unaware of how schemas are stored or fetched, and reports failures through any
/// [`Display`] error type of the caller's choosing.
pub struct ExplorerBuilder<P> {
    config: TuiConfig,
    models: Vec<DataModel>,
    schema_provider: P,
}

impl<P> ExplorerBuilder<P> {
    /// Prepares the explorer over `models`, resolving schemas through `schema_provider`.
    pub const fn new(config: TuiConfig, models: Vec<DataModel>, schema_provider: P) -> ExplorerBuilder<P> {
        ExplorerBuilder {
            config,
            models,
            schema_provider,
        }
    }

    /// Runs the explorer until the user quits.
    ///
    /// # Errors
    /// Returns a [`TuiError`](crate::error::TuiError) when the terminal cannot be driven.
    pub fn run<PErr>(self) -> Result<()>
    where
        P: Fn(&DataModel) -> Result<Value, PErr>,
        PErr: Display,
    {
        let ExplorerBuilder {
            config,
            models,
            schema_provider,
        } = self;
        let theme = *config.theme();
        let runner = TuiRunner::new(config);
        let mut state = ExplorerState::new(models);

        runner.run(|terminal, _| {
            loop {
                terminal.draw(|frame| ui(frame, &mut state, &theme))?;

                if event::poll(Duration::from_millis(50))?
                    && let Event::Key(key) = event::read()?
                    && key.kind == KeyEventKind::Press
                {
                    if key.code == KeyCode::Enter
                        && state.current_screen == ExplorerScreen::ModelSelection
                        && state.error_message.is_none()
                        && let Some(model) = state.model_picker.selected_entry().cloned()
                    {
                        match schema_provider(&model) {
                            Ok(json) => ExplorerReducer::apply(&mut state, ExplorerAction::SchemaLoaded(model, json, theme)),
                            Err(error) => state.error_message = Some(error.to_string()),
                        }
                    } else {
                        ExplorerReducer::apply(&mut state, ExplorerAction::KeyPress(key));
                    }
                }

                if state.should_quit {
                    break;
                }
            }

            Ok(())
        })
    }
}
