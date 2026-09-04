use crate::{
    announcement::announce_schedule,
    payload::SchedulerPayload,
    sleep::{SleepOutcome, interruptible_sleep},
    wait::{Jitter, apply_jitter, next_wait},
};
use cassiopeia_common::{
    run::{RunCount, RunNumber},
    signal::Signal,
};
use cassiopeia_manifest::schedule::Schedule;
use cassiopeia_reporter::reporter::Reporter;
use std::{
    convert::Infallible,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::spawn,
    time::Instant,
};

/// The signal type the scheduler emits: run payloads, never an error, because every fallible part
/// of a schedule is resolved when the manifest is parsed.
type SchedulerSignal = Signal<SchedulerPayload, Infallible>;

/// Spawns the scheduler thread and returns the channel it drives.
///
/// A `None` schedule is the manifest's "run once" case: the thread emits a single run and stops.
/// A `Some` schedule repeats according to its trigger until its bound (repeat count or total
/// duration) is reached, the trigger has no further runs, or `shutdown` is set.
///
/// The channel is a rendezvous ([`mpsc::sync_channel`] of size zero): sending a run blocks until the
/// consumer takes it, so the next wait is only computed once the current run has been picked up.
pub fn spawn_scheduler_thread(schedule: Option<Schedule>, reporter: &'static dyn Reporter, shutdown: &'static AtomicBool) -> Receiver<SchedulerSignal> {
    let (sender, receiver) = mpsc::sync_channel(0);

    spawn(move || run(schedule, reporter, shutdown, &sender));

    receiver
}

/// Drives the whole schedule on the spawned thread, bracketing the runs with `Start` and `Stop`.
fn run(schedule: Option<Schedule>, reporter: &'static dyn Reporter, shutdown: &'static AtomicBool, sender: &SyncSender<SchedulerSignal>) {
    if sender.send(Signal::Start).is_err() {
        return;
    }

    match schedule {
        None => {
            let payload = SchedulerPayload {
                run_number: RunNumber::new(1),
                total_runs: Some(RunCount::new(1)),
            };
            // A dropped receiver is nothing to recover from here, fall through to `Stop`.
            let _ = sender.send(Signal::Data(payload));
        }
        Some(schedule) => drive_repeating(&schedule, reporter, shutdown, sender),
    }

    // Best-effort: if the receiver is already gone there is nothing left to signal.
    let _ = sender.send(Signal::Stop);
}

/// Emits runs for a repeating schedule until a bound is hit, the trigger is exhausted, or shutdown.
fn drive_repeating(schedule: &Schedule, reporter: &'static dyn Reporter, shutdown: &'static AtomicBool, sender: &SyncSender<SchedulerSignal>) {
    announce_schedule(schedule, reporter);

    let total_runs = schedule.repeat().map(|count| RunCount::new(count.get()));
    let deadline = schedule.duration().map(|limit| Instant::now() + limit);
    let mut run_number: u32 = 0;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        if let Some(deadline) = deadline
            && Instant::now() >= deadline
        {
            reporter.info("Schedule duration expired");
            break;
        }

        if let Some(max) = total_runs
            && run_number >= max.get()
        {
            break;
        }

        // The first run fires immediately; every later run waits for its trigger first.
        if run_number > 0 {
            let Some(wait) = next_wait(schedule.trigger()) else {
                break;
            };
            let wait = apply_jitter(wait, Jitter::new(*schedule.jitter()));

            if !wait.is_zero() {
                match interruptible_sleep(wait, shutdown) {
                    SleepOutcome::Elapsed => {}
                    SleepOutcome::Interrupted => break,
                }
            }
        }

        run_number += 1;

        let payload = SchedulerPayload {
            run_number: RunNumber::new(run_number),
            total_runs,
        };

        if sender.send(Signal::Data(payload)).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{payload::SchedulerPayload, stage::spawn_scheduler_thread};
    use cassiopeia_common::{
        run::{RunCount, RunNumber},
        signal::Signal,
    };
    use cassiopeia_manifest::schedule::{Schedule, trigger::Trigger};
    use cassiopeia_reporter::backend::noop::NoopReporter;
    use std::{
        convert::Infallible,
        num::NonZeroU32,
        sync::{atomic::AtomicBool, mpsc::Receiver},
        time::Duration,
    };

    /// A process-lifetime reporter for the `&'static` the stage needs; it records nothing.
    fn reporter() -> &'static NoopReporter {
        static REPORTER: NoopReporter = NoopReporter::new();
        &REPORTER
    }

    /// A process-lifetime, never-set shutdown flag.
    fn shutdown() -> &'static AtomicBool {
        static SHUTDOWN: AtomicBool = AtomicBool::new(false);
        &SHUTDOWN
    }

    /// A process-lifetime shutdown flag that is already set before any run starts.
    fn already_shut_down() -> &'static AtomicBool {
        static SHUTDOWN: AtomicBool = AtomicBool::new(true);
        &SHUTDOWN
    }

    /// Collects a scheduler channel into `(saw_start, runs, saw_stop)` for assertions.
    fn drain(receiver: Receiver<Signal<SchedulerPayload, Infallible>>) -> (bool, Vec<(RunNumber, Option<RunCount>)>, bool) {
        let mut saw_start = false;
        let mut saw_stop = false;
        let mut runs = Vec::new();

        for signal in receiver {
            match signal {
                Signal::Start => saw_start = true,
                Signal::Stop => saw_stop = true,
                Signal::Data(payload) => runs.push((payload.run_number, payload.total_runs)),
                Signal::Meta(_) => {}
                Signal::Error(never) => match never {},
            }
        }

        (saw_start, runs, saw_stop)
    }

    #[test]
    fn a_manifest_without_a_schedule_runs_exactly_once() {
        let receiver = spawn_scheduler_thread(None, reporter(), shutdown());

        let (saw_start, runs, saw_stop) = drain(receiver);

        assert!(saw_start);
        assert!(saw_stop);
        assert_eq!(runs, vec![(RunNumber::new(1), Some(RunCount::new(1)))]);
    }

    #[test]
    fn a_bounded_interval_schedule_runs_the_requested_number_of_times() {
        let schedule = Schedule::builder()
            .trigger(Trigger::Every(Duration::from_millis(1)))
            .repeat(NonZeroU32::new(2))
            .build();

        let receiver = spawn_scheduler_thread(Some(schedule), reporter(), shutdown());

        let (saw_start, runs, saw_stop) = drain(receiver);

        assert!(saw_start);
        assert!(saw_stop);
        assert_eq!(
            runs,
            vec![(RunNumber::new(1), Some(RunCount::new(2))), (RunNumber::new(2), Some(RunCount::new(2))),]
        );
    }

    #[test]
    fn a_schedule_already_under_shutdown_emits_no_runs() {
        let schedule = Schedule::builder().trigger(Trigger::Every(Duration::from_millis(1))).build();

        let receiver = spawn_scheduler_thread(Some(schedule), reporter(), already_shut_down());

        let (saw_start, runs, saw_stop) = drain(receiver);

        assert!(saw_start);
        assert!(saw_stop);
        assert!(runs.is_empty());
    }
}
