//! The Cassiopeia CLI's shared terminal presentation language.
//!
//! One visual vocabulary consumed by both output paths: the one-shot report renderers (which return
//! a `String` for a handler to print) and the live streaming reporter (indicatif bars and log lines).
//! It sits below both `cassiopeia-cli` and `cassiopeia-reporter` so the single source of truth for
//! the constellation palette is shared rather than copied.
//!
//! One concept per module: [`palette`] the semantic colours, [`symbol`] the status glyphs,
//! [`connector`] the box-drawing glyphs that hang detail lines under a headline, [`rendering`] the
//! colour-versus-plain decision per output stream, [`paint`] the styling and middot-joining helpers,
//! [`field`] the `label value` fragment, and [`byte_size`] the human-readable byte formatter.

pub mod byte_size;
pub mod connector;
pub mod field;
pub mod paint;
pub mod palette;
pub mod rendering;
pub mod symbol;
