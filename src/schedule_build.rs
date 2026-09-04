use crate::cli::schedule::ScheduleArgs;
use cassiopeia_manifest::schedule::{Schedule, retry::RetryPolicy, trigger::Trigger};

/// Builds the manifest schedule the `map` command's schedule flags describe, if any.
///
/// Every domain decision (which trigger wins, when a retry policy exists, when the schedule
/// exists at all) lives on the manifest types; this adapter only reads clap's flags into their
/// plain-value constructors.
pub fn schedule_from_args(args: &ScheduleArgs) -> Option<Schedule> {
    let trigger = Trigger::first_present(args.every, args.cron.clone().map(Box::new), args.at.clone());
    let retry = RetryPolicy::new(args.retry, args.retry_backoff);

    Schedule::assemble(trigger, args.repeat, args.duration, args.jitter, retry)
}
