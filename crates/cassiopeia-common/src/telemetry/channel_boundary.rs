//! The stage boundary an instrumented channel sits on, so a queue's measurements name the two
//! stages it connects rather than appearing as an anonymous row.

use crate::stage::Stage;

/// The pair of stages a pipeline channel bridges: the stage that produces into the queue and the
/// stage that consumes from it.
///
/// This is measurement metadata, not topology logic; it records which two stages a queue connected
/// so the run summary can name each channel. The consumer is optional: the final stage's output
/// queue drains to the run's sink rather than into another stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelBoundary {
    /// The stage whose output the queue holds.
    pub producer: Stage,
    /// The stage that drains the queue, or `None` when it drains to the run's terminal sink.
    pub consumer: Option<Stage>,
}

impl ChannelBoundary {
    /// A boundary between two named stages.
    #[must_use]
    pub const fn between(producer: Stage, consumer: Stage) -> ChannelBoundary {
        ChannelBoundary {
            producer,
            consumer: Some(consumer),
        }
    }

    /// A boundary whose queue drains to the run's terminal sink rather than a downstream stage.
    #[must_use]
    pub const fn to_sink(producer: Stage) -> ChannelBoundary {
        ChannelBoundary { producer, consumer: None }
    }
}
