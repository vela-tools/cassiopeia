//! Terminal implementation of the Reporter using indicatif.
//!
//! The backend is split by concern: colours and symbols come from the shared
//! [`palette`](cassiopeia_terminal_style::palette) and [`symbol`](cassiopeia_terminal_style::symbol),
//! [`column`] the live-rendered progress-bar columns (rate, warnings, latency,
//! elapsed), [`diagnostic_lines`] one diagnostic rendered as terminal lines,
//! [`style`] the `ProgressStyle` builders for each stage display,
//! [`stage`] the per-stage shared state and its animation thread, [`registry`]
//! the ordered map of stage entries, [`entry_handle`] one running stage's view
//! of its entry, [`summary`] the end-of-run telemetry tables, and [`reporter`]
//! the [`reporter::TerminalReporter`] that ties them together.

pub mod column;
pub(crate) mod diagnostic_lines;
pub(crate) mod entry_handle;
pub(crate) mod registry;
pub mod reporter;
pub mod stage;
pub mod style;
pub mod summary;
