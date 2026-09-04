use crate::run::{RunCount, RunNumber};
use derive_more::IsVariant;
use serde::{Deserialize, Serialize};

/// Metadata signals that propagate through the pipeline without being transformed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MetaSignal {
    /// The total number of inputs being processed in this pipeline run.
    InputCount(usize),
    /// Schedule context for this pipeline run.
    ScheduleRun {
        /// 1-based index of this run.
        run_number: RunNumber,
        /// Total number of runs if bounded, `None` if unbounded.
        total_runs: Option<RunCount>,
    },
}

/// The envelope carried between pipeline stages: a data payload, an error, or control.
///
/// Generic over the payload type `T` and the error type `E`, so each stage can carry its own
/// record and error types through the same channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, IsVariant)]
pub enum Signal<T, E> {
    /// The start of a new stream, batch, or processing cycle.
    Start,
    /// A stream or batch finished successfully.
    Stop,
    /// A data payload of type `T`.
    Data(T),
    /// An error of type `E` produced while processing.
    Error(E),
    /// Pipeline metadata forwarded unchanged.
    Meta(MetaSignal),
}

impl<T, E> Signal<T, E> {
    /// Applies `f` to the payload when the signal carries data; every other variant passes
    /// through unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use cassiopeia_common::signal::Signal;
    /// let signal: Signal<i32, ()> = Signal::Data(10);
    /// let mapped = signal.map(|x| x * 2);
    /// assert_eq!(mapped, Signal::Data(20));
    /// ```
    pub fn map<U, F>(self, f: F) -> Signal<U, E>
    where
        F: FnOnce(T) -> U,
    {
        match self {
            Signal::Start => Signal::Start,
            Signal::Stop => Signal::Stop,
            Signal::Data(val) => Signal::Data(f(val)),
            Signal::Error(err) => Signal::Error(err),
            Signal::Meta(meta) => Signal::Meta(meta),
        }
    }

    /// Transforms the error payload of the signal using the provided function.
    pub fn map_err<F, U>(self, f: F) -> Signal<T, U>
    where
        F: FnOnce(E) -> U,
    {
        match self {
            Signal::Start => Signal::Start,
            Signal::Stop => Signal::Stop,
            Signal::Data(val) => Signal::Data(val),
            Signal::Error(err) => Signal::Error(f(err)),
            Signal::Meta(meta) => Signal::Meta(meta),
        }
    }

    /// Converts the signal into an `Option<T>`, discarding errors, control, and meta signals.
    #[must_use]
    pub fn into_data(self) -> Option<T> {
        match self {
            Signal::Data(val) => Some(val),
            Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_) => None,
        }
    }
}

impl<T, E> From<T> for Signal<T, E> {
    fn from(val: T) -> Self {
        Signal::Data(val)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        run::{RunCount, RunNumber},
        signal::{MetaSignal, Signal},
    };

    #[test]
    fn mapping_data_applies_the_function_to_the_payload() {
        let signal: Signal<i32, String> = Signal::Data(5);
        let mapped = signal.map(|x| x * 2);
        assert_eq!(mapped, Signal::Data(10));
    }

    #[test]
    fn mapping_a_control_signal_leaves_it_unchanged() {
        let signal: Signal<i32, String> = Signal::Start;
        let mapped = signal.map(|x| x * 2);
        assert_eq!(mapped, Signal::Start);
    }

    #[test]
    fn mapping_the_error_applies_the_function_only_to_the_error_payload() {
        let signal: Signal<i32, i32> = Signal::Error(1);
        let mapped = signal.map_err(|e| e + 41);
        assert_eq!(mapped, Signal::Error(42));
    }

    #[test]
    fn mapping_a_meta_signal_leaves_it_unchanged() {
        let signal: Signal<i32, String> = Signal::Meta(MetaSignal::InputCount(5));
        let mapped = signal.map(|x| x * 2);
        assert_eq!(mapped, Signal::Meta(MetaSignal::InputCount(5)));
    }

    #[test]
    fn the_generated_predicates_report_the_active_variant() {
        let data: Signal<i32, ()> = Signal::Data(1);
        let start: Signal<i32, ()> = Signal::Start;
        let stop: Signal<i32, ()> = Signal::Stop;
        let error: Signal<i32, i32> = Signal::Error(500);
        let meta: Signal<i32, ()> = Signal::Meta(MetaSignal::InputCount(3));

        assert!(data.is_data());
        assert!(start.is_start());
        assert!(stop.is_stop());
        assert!(error.is_error());
        assert!(meta.is_meta());

        assert!(!data.is_start());
        assert!(!start.is_data());
        assert!(!meta.is_data());
    }

    #[test]
    fn only_a_data_signal_yields_its_payload() {
        let data: Signal<i32, ()> = Signal::Data(7);
        let meta: Signal<i32, ()> = Signal::Meta(MetaSignal::InputCount(1));

        assert_eq!(data.into_data(), Some(7));
        assert_eq!(meta.into_data(), None);
    }

    #[test]
    fn a_schedule_run_meta_signal_round_trips_through_json() {
        let meta = MetaSignal::ScheduleRun {
            run_number: RunNumber::new(2),
            total_runs: Some(RunCount::new(5)),
        };
        let encoded = serde_json::to_string(&meta).unwrap();

        assert_eq!(serde_json::from_str::<MetaSignal>(&encoded).unwrap(), meta);
    }
}
