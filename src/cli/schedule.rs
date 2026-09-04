use cassiopeia_manifest::schedule::time_of_day::TimeOfDay;
use clap::Args;
use cron::{Schedule as CronSchedule, error::Error as CronError};
use std::{num::NonZeroU32, str::FromStr, time::Duration};

/// Parses a cron expression into a schedule, used as a clap value parser.
fn parse_cron(value: &str) -> Result<CronSchedule, CronError> {
    CronSchedule::from_str(value)
}

/// Arguments controlling repeated scheduling of a run.
#[derive(Args, Debug)]
pub struct ScheduleArgs {
    /// Run at a fixed interval, timezone-independent (e.g. 30s, 5m, 2h).
    #[arg(
        long,
        help_heading = "Schedule",
        value_name = "INTERVAL",
        value_parser = humantime::parse_duration,
        conflicts_with_all = ["cron", "at"]
    )]
    pub every: Option<Duration>,

    /// Run on a cron schedule, evaluated in UTC (e.g. "0 0 */5 * * *").
    ///
    /// Six fields with seconds precision (second, minute, hour, day-of-month, month, day-of-week)
    /// plus an optional seventh year field. A five-field crontab line has no seconds field and will
    /// not parse.
    #[arg(
        long,
        help_heading = "Schedule",
        value_name = "EXPRESSION",
        value_parser = parse_cron,
        conflicts_with_all = ["every", "at"]
    )]
    pub cron: Option<CronSchedule>,

    /// Run at fixed times of day, written HH:MM and evaluated in local time (e.g. 14:00,18:00).
    #[arg(
        long,
        help_heading = "Schedule",
        value_name = "TIMES",
        value_delimiter = ',',
        conflicts_with_all = ["every", "cron"]
    )]
    pub at: Vec<TimeOfDay>,

    /// Maximum number of runs.
    #[arg(long, help_heading = "Schedule", value_name = "COUNT")]
    pub repeat: Option<NonZeroU32>,

    /// Stop scheduling after this duration (e.g. 2h).
    #[arg(
        long = "schedule-duration",
        help_heading = "Schedule",
        value_name = "DURATION",
        value_parser = humantime::parse_duration
    )]
    pub duration: Option<Duration>,

    /// Random jitter added to the wait between runs (e.g. 10s).
    #[arg(long, help_heading = "Schedule", value_name = "DURATION", value_parser = humantime::parse_duration)]
    pub jitter: Option<Duration>,

    /// Retry a failed run up to this many times.
    #[arg(long, help_heading = "Schedule", value_name = "ATTEMPTS")]
    pub retry: Option<NonZeroU32>,

    /// Backoff between retry attempts (default 5s).
    #[arg(
        long,
        help_heading = "Schedule",
        value_name = "DURATION",
        value_parser = humantime::parse_duration,
        requires = "retry"
    )]
    pub retry_backoff: Option<Duration>,
}
