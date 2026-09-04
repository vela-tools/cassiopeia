use anstyle::Effects;
use cassiopeia_terminal_style::palette::{ACCENT, ERROR, FRAME, HIGHLIGHT, MUTED, PRIMARY, SUCCESS};
use clap::builder::styling::Styles;

/// Cassiopeia constellation-inspired clap theme, built from the shared [`palette`](cassiopeia_terminal_style::palette).
///
/// Each clap role maps to a semantic palette colour, with clap-specific effects layered on:
/// - **Headers**: [`PRIMARY`] gold, underlined
/// - **Usage**: [`PRIMARY`] gold, bold only (no underline, to stay subordinate to section headers)
/// - **Literals** (flags, commands): [`ACCENT`] steel blue, bold
/// - **Placeholders** (value names): [`MUTED`] slate
/// - **Context** (`[default: …]`, `[possible values: …]`): [`FRAME`] grey
/// - **Context values**: [`HIGHLIGHT`] peach
/// - **Valid suggestions**: [`SUCCESS`] green, bold
/// - **Invalid / error**: [`ERROR`] red, bold
pub const THEME: Styles = Styles::styled()
    .header(PRIMARY.effects(Effects::BOLD.insert(Effects::UNDERLINE)))
    .usage(PRIMARY)
    .literal(ACCENT.effects(Effects::BOLD))
    .placeholder(MUTED)
    .context(FRAME)
    .context_value(HIGHLIGHT)
    .valid(SUCCESS.effects(Effects::BOLD))
    .invalid(ERROR.effects(Effects::BOLD))
    .error(ERROR.effects(Effects::BOLD));
