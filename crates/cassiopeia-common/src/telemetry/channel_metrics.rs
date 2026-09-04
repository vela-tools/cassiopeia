//! Point-in-time snapshot of one instrumented channel's queue and backpressure counters.

use crate::{
    channel::ChannelMetrics,
    telemetry::{channel_boundary::ChannelBoundary, nanos::from_nanos},
};
use std::{sync::atomic::Ordering, time::Duration};

/// Channel queue and backpressure measurements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChannelSnapshot {
    /// The stage boundary this queue sits on, naming the two stages it connects.
    pub boundary: Option<ChannelBoundary>,
    pub capacity: Option<usize>,
    pub depth: usize,
    pub high_water_depth: usize,
    pub sent: u64,
    pub received: u64,
    pub send_wait: Duration,
    pub receive_wait: Duration,
    pub time_at_capacity: Duration,
    pub blocked_sends: u64,
    pub disconnected: u64,
}

impl ChannelMetrics {
    /// Takes a point-in-time snapshot without changing counters.
    #[must_use]
    pub fn snapshot(&self) -> ChannelSnapshot {
        // A non-zero marker means the queue is full right now, so the still-open window counts too.
        let open_window = match self.capacity_since_ns.load(Ordering::Relaxed) {
            0 => 0,
            since => self.now_ns().saturating_sub(since),
        };
        let at_capacity = self.time_at_capacity_ns.load(Ordering::Relaxed).saturating_add(open_window);
        ChannelSnapshot {
            boundary: self.boundary,
            capacity: self.capacity,
            depth: self.depth.load(Ordering::Relaxed),
            high_water_depth: self.peak_depth.load(Ordering::Relaxed),
            sent: self.sent.load(Ordering::Relaxed),
            received: self.received.load(Ordering::Relaxed),
            send_wait: from_nanos(self.send_wait_ns.load(Ordering::Relaxed)),
            receive_wait: from_nanos(self.receive_wait_ns.load(Ordering::Relaxed)),
            time_at_capacity: from_nanos(at_capacity),
            blocked_sends: self.blocked_sends.load(Ordering::Relaxed),
            disconnected: self.disconnected.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        channel::{ChannelPolicy, channel, channel_with_telemetry},
        stage::Stage,
        telemetry::channel_boundary::ChannelBoundary,
    };
    use std::num::NonZeroUsize;

    #[test]
    fn a_snapshot_carries_the_channels_stage_boundary() {
        let boundary = ChannelBoundary::between(Stage::Ingestor, Stage::Expander);
        let (sender, _receiver) = channel_with_telemetry::<u8>(ChannelPolicy::Unbounded, None, Some(boundary));
        assert_eq!(sender.metrics().snapshot().boundary, Some(boundary));
    }

    #[test]
    fn a_snapshot_reports_capacity_and_the_high_water_depth() {
        let capacity = NonZeroUsize::new(2).unwrap();
        let (sender, receiver) = channel::<u8>(ChannelPolicy::Bounded(capacity));
        sender.send(1).unwrap();
        sender.send(2).unwrap();
        let snapshot = sender.metrics().snapshot();
        assert_eq!(snapshot.capacity, Some(2));
        assert_eq!(snapshot.high_water_depth, 2);
        assert_eq!(snapshot.sent, 2);
        assert_eq!(receiver.recv().unwrap(), 1);
    }
}
