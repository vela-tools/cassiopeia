use crate::{
    config::TuiConfig,
    error::Result,
    notification::Notification,
    runner::TuiRunner,
    screen::wizard::{
        events::{WizardAction, WizardReducer},
        state::WizardState,
        step::WizardStep,
        ui::ui,
    },
};
use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
use cassiopeia_ngsi_ld::data_model::DataModel;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use serde_json::Value;
use std::{fmt::Display, time::Duration};

/// Configures and launches the mapping wizard.
///
/// `schema_provider` resolves a data model to its dereferenced JSON Schema, and `saver` writes a
/// finished mapping; both are closures so the TUI stays unaware of storage and fetching, and both
/// report failures through any [`Display`] error type of the caller's choosing.
pub struct WizardBuilder<P, S> {
    config: TuiConfig,
    models: Vec<DataModel>,
    schema_provider: P,
    saver: S,
}

impl<P, S> WizardBuilder<P, S> {
    /// Prepares the wizard over `models`, resolving schemas through `schema_provider` and writing
    /// the result through `saver`.
    pub const fn new(config: TuiConfig, models: Vec<DataModel>, schema_provider: P, saver: S) -> WizardBuilder<P, S> {
        WizardBuilder {
            config,
            models,
            schema_provider,
            saver,
        }
    }

    /// Runs the wizard until the user quits or a mapping is saved.
    ///
    /// # Errors
    /// Returns a [`TuiError`](crate::error::TuiError) when the terminal cannot be driven.
    pub fn run<PErr, SErr>(self) -> Result<()>
    where
        P: Fn(&DataModel) -> Result<Value, PErr>,
        PErr: Display,
        S: Fn(Mapping) -> Result<(), SErr>,
        SErr: Display,
    {
        let WizardBuilder {
            config,
            models,
            schema_provider,
            saver,
        } = self;
        let theme = *config.theme();
        let runner = TuiRunner::new(config);
        let mut state = WizardState::new(models);

        runner.run(|terminal, _| {
            loop {
                terminal.draw(|frame| ui(frame, &mut state, &theme))?;

                if event::poll(Duration::from_millis(50))?
                    && let Event::Key(key) = event::read()?
                    && key.kind == KeyEventKind::Press
                {
                    if key.code == KeyCode::Enter {
                        match state.step {
                            WizardStep::ModelSelection => {
                                if let Some(choice) = state.model_picker.selected_entry().cloned() {
                                    match choice.existing() {
                                        None => {
                                            state.new_model_input.clear();
                                            state.step = WizardStep::DataModelNameInput;
                                        }
                                        Some(model) => {
                                            state.target_model = Some(model.clone());
                                            state.save_filename = format!("{model}.json5");
                                            match schema_provider(model) {
                                                Ok(json) => {
                                                    state.load_schema(&json);
                                                    state.step = WizardStep::IdentityForm;
                                                }
                                                Err(error) => state.notification = Some(Notification::new(format!("Error: {error}"), theme.error, 60)),
                                            }
                                        }
                                    }
                                }
                            }
                            WizardStep::SavePreview => {
                                // Clone the target so the state can be borrowed mutably while building.
                                if let Some(model) = state.target_model.clone() {
                                    let mut template_runner = TemplateRunner::new();
                                    match state.build_mapping(&model, &mut template_runner) {
                                        Ok(mapping) => match saver(mapping) {
                                            Ok(()) => state.should_quit = true,
                                            Err(error) => state.notification = Some(Notification::new(format!("Save Error: {error}"), theme.error, 60)),
                                        },
                                        Err(error) => state.notification = Some(Notification::new(format!("Error: {error}"), theme.error, 60)),
                                    }
                                } else {
                                    state.notification = Some(Notification::new("Error: no data model selected".to_string(), theme.error, 60));
                                }
                            }
                            WizardStep::DataModelNameInput | WizardStep::IdentityForm | WizardStep::AttributeList | WizardStep::AttributeEditor => {
                                WizardReducer::apply(&mut state, WizardAction::KeyPress(key, theme));
                            }
                        }
                    } else {
                        WizardReducer::apply(&mut state, WizardAction::KeyPress(key, theme));
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
