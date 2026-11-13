use cassiopeia_sensorthings::{cursor_name::CursorName, time_point::TimePoint};
use cassiopeia_sensorthings_cursor::reset::{IdText, ResetScope};
use clap::{Args, Parser, Subcommand};

/// The `sensorthings cursor` command group.
#[derive(Parser, Debug)]
pub struct CursorCommand {
    /// The cursor operation to perform.
    #[command(subcommand)]
    pub command: CursorSubcommand,
}

/// Operations on the cursors that keep how far incremental SensorThings reads have read, in the
/// cursors folder of the `cursors.folder` setting, where runs keep them.
#[derive(Subcommand, Debug)]
pub enum CursorSubcommand {
    /// List the SensorThings cursors in the cursors folder.
    ///
    /// Prints one row per cursor: its name, the service root its positions were read from, the
    /// number of data streams it holds a position for, and the oldest and newest of their high-water
    /// marks, `-` for a cursor without positions. A cursor of another kind of input is left out.
    List,

    /// Show the position a cursor keeps for each data stream.
    ///
    /// Prints the service root, then one row per data stream: its id, its high-water mark, the
    /// number of Observations it has seen within the lookback, the start of the last cycle that
    /// listed it, and the start of the first cycle that no longer listed it, `-` while it is listed.
    #[command(arg_required_else_help = true)]
    Show(CursorArg),

    /// Reset a cursor, so the next cycle reads its data streams again from a chosen point.
    ///
    /// Without `--to`, removes the positions, so the next cycle starts each data stream at the
    /// input's `initial` point. With `--to`, sets their high-water marks to the target and forgets
    /// the Observations they have seen, so the next cycle reads each from the target minus the
    /// lookback.
    #[command(arg_required_else_help = true)]
    Reset(ResetArgs),
}

/// The cursor a command applies to.
#[derive(Args, Debug)]
pub struct CursorArg {
    /// The cursor, as an input's `read.incremental.cursor` names it, such as `rivm-air`.
    #[arg(value_name = "CURSOR")]
    pub cursor: CursorName,
}

/// The arguments of `sensorthings cursor reset`.
#[derive(Args, Debug)]
pub struct ResetArgs {
    #[command(flatten)]
    pub cursor: CursorArg,

    /// Reset only the data stream with this id, as `cursor show` prints it; every data stream when
    /// absent.
    #[arg(long, value_name = "ID")]
    pub datastream: Option<IdText>,

    /// Set the high-water marks to this RFC 3339 instant, or to this span before now, such as `1h`;
    /// remove the positions when absent. A target after now is refused.
    #[arg(long, value_name = "INSTANT|DURATION")]
    pub to: Option<TimePoint>,
}

impl ResetArgs {
    /// What the reset applies to: the data stream `--datastream` names, or every one.
    #[must_use]
    pub fn scope(&self) -> ResetScope {
        // The scope owns its id text, while the parsed command line keeps its own.
        self.datastream.clone().map_or(ResetScope::Store, ResetScope::Datastream)
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::{
        Cli,
        Commands,
        sensorthings::SensorThingsSubcommand,
        sensorthings_cursor::{CursorSubcommand, ResetArgs},
    };
    use cassiopeia_sensorthings::time_point::TimePoint;
    use cassiopeia_sensorthings_cursor::reset::{IdText, ResetScope};
    use clap::Parser;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    /// The `sensorthings cursor` subcommand of `cassiopeia sensorthings cursor` followed by
    /// `arguments`.
    fn cursor_command(arguments: &[&str]) -> CursorSubcommand {
        let command_line = ["cassiopeia", "sensorthings", "cursor"].iter().chain(arguments);
        let Commands::SensorThings(command) = Cli::try_parse_from(command_line).unwrap().command else {
            panic!("expected the sensorthings command");
        };
        let SensorThingsSubcommand::Cursor(command) = command.command else {
            panic!("expected the cursor command");
        };
        command.command
    }

    fn reset_args(arguments: &[&str]) -> ResetArgs {
        let CursorSubcommand::Reset(args) = cursor_command(&[&["reset"], arguments].concat()) else {
            panic!("expected the reset command");
        };
        args
    }

    /// The message the command line `cassiopeia sensorthings cursor` followed by `arguments` is
    /// refused with.
    fn refusal(arguments: &[&str]) -> String {
        let command_line = ["cassiopeia", "sensorthings", "cursor"].iter().chain(arguments);
        Cli::try_parse_from(command_line).unwrap_err().to_string()
    }

    #[test]
    fn list_takes_no_arguments_and_show_takes_a_cursor() {
        assert!(matches!(cursor_command(&["list"]), CursorSubcommand::List));
        let CursorSubcommand::Show(shown) = cursor_command(&["show", "rivm-air"]) else {
            panic!("expected the show command");
        };

        assert_eq!(shown.cursor.as_str(), "rivm-air");
    }

    #[test]
    fn the_reset_command_reads_its_cursor_data_stream_and_target() {
        let args = reset_args(&["rivm-air", "--datastream", "t-5", "--to", "1h"]);

        assert_eq!(args.cursor.cursor.as_str(), "rivm-air");
        assert_eq!(args.scope(), ResetScope::Datastream(IdText::from("t-5".to_owned())));
        assert_eq!(args.to, Some(TimePoint::Before(Duration::from_hours(1))));
        assert_eq!(
            reset_args(&["rivm-air", "--to", "2026-11-12T08:00:00Z"]).to.map(|to| to.to_string()),
            Some("2026-11-12T08:00:00Z".to_owned())
        );
    }

    #[test]
    fn a_reset_without_options_applies_to_every_data_stream_without_a_target() {
        let args = reset_args(&["rivm-air"]);

        assert_eq!(args.scope(), ResetScope::Store);
        assert_eq!(args.to, None);
    }

    #[test]
    fn a_malformed_cursor_name_is_refused_with_its_reason() {
        let error = refusal(&["show", "Rivm"]);

        assert!(error.contains("a cursor name holds only `a-z`, `0-9` and `-`, not `R`"), "{error}");
    }

    #[test]
    fn an_unreadable_target_is_refused_with_its_reason() {
        let error = refusal(&["reset", "rivm-air", "--to", "soon"]);

        assert!(error.contains("an RFC 3339 instant or a span such as `24h`, not `soon`"), "{error}");
    }
}
