use cassiopeia_manifest::schedule::{Schedule, trigger::Trigger};
use cassiopeia_reporter::reporter::MessageLog;
use humantime::format_duration;

/// Reports a one-line, human-readable summary of how often a schedule runs, once at start-up.
pub(crate) fn announce_schedule(schedule: &Schedule, reporter: &dyn MessageLog) {
    let repeat_suffix = match schedule.repeat() {
        Some(count) => format!(" ({count} runs)"),
        None => String::new(),
    };

    match schedule.trigger() {
        Trigger::Every(interval) => reporter.info(&format!("Runs every {}{repeat_suffix}", format_duration(*interval))),
        Trigger::Cron(cron) => reporter.info(&format!("Runs on cron schedule: {cron}{repeat_suffix}")),
        Trigger::At(times) => {
            let listed = times.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
            reporter.info(&format!("Runs daily at {listed}{repeat_suffix}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::announcement::announce_schedule;
    use cassiopeia_manifest::schedule::{Schedule, time_of_day::TimeOfDay, trigger::Trigger};
    use cassiopeia_reporter::reporter::MessageLog;
    use std::{num::NonZeroU32, str::FromStr, sync::Mutex, time::Duration};

    /// Captures only the informational lines `announce_schedule` emits, so a test can assert the
    /// exact summary text.
    #[derive(Default)]
    struct RecordingLog {
        info_lines: Mutex<Vec<String>>,
    }

    impl MessageLog for RecordingLog {
        fn info(&self, message: &str) {
            self.info_lines.lock().unwrap().push(message.to_string());
        }
        fn success(&self, _message: &str) {}
        fn debug(&self, _message: &str) {}
        fn step(&self, _current: usize, _total: usize, _message: &str) {}
        fn raw_log(&self, _message: &str) {}
    }

    fn announce(schedule: &Schedule) -> String {
        let log = RecordingLog::default();
        announce_schedule(schedule, &log);
        let lines = log.info_lines.lock().unwrap();
        assert_eq!(lines.len(), 1);
        lines[0].clone()
    }

    #[test]
    fn an_unbounded_interval_schedule_is_announced_without_a_run_count() {
        let schedule = Schedule::builder().trigger(Trigger::Every(Duration::from_mins(1))).build();

        let line = announce(&schedule);

        assert!(line.starts_with("Runs every "));
        assert!(!line.contains("runs)"));
    }

    #[test]
    fn a_bounded_interval_schedule_appends_the_run_count() {
        let schedule = Schedule::builder()
            .trigger(Trigger::Every(Duration::from_mins(1)))
            .repeat(NonZeroU32::new(3))
            .build();

        assert!(announce(&schedule).ends_with("(3 runs)"));
    }

    #[test]
    fn a_fixed_time_schedule_lists_every_time() {
        let schedule = Schedule::builder()
            .trigger(Trigger::At(vec![TimeOfDay::from_str("08:00").unwrap(), TimeOfDay::from_str("20:00").unwrap()]))
            .build();

        let line = announce(&schedule);

        assert!(line.starts_with("Runs daily at "));
        assert!(line.contains("08:00"));
        assert!(line.contains("20:00"));
    }
}
