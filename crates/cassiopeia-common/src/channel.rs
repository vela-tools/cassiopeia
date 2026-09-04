use crate::telemetry::{channel_boundary::ChannelBoundary, nanos::to_nanos, run::RunTelemetry};
use derive_where::derive_where;
use smart_default::SmartDefault;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, RecvError, SendError, TryRecvError},
    },
    time::{Duration, Instant},
};

/// Whether a pipeline handoff may grow with the producer or back-pressures it at a fixed depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPolicy {
    /// Let the producer run ahead of the consumer, favouring throughput and stage overlap.
    Unbounded,
    /// Block the producer once this many messages are waiting, bounding in-flight memory.
    Bounded(NonZeroUsize),
}

/// A policy-neutral sending half of a pipeline channel.
///
/// `std::sync::mpsc` exposes different sender types for bounded and unbounded channels. This wrapper
/// keeps that implementation detail out of stage and plugin APIs while retaining the same `send`
/// semantics for both policies.
#[derive(Debug)]
#[derive_where(Clone)]
pub enum ChannelSender<T> {
    Unbounded(mpsc::Sender<T>, Arc<ChannelMetrics>),
    Bounded(mpsc::SyncSender<T>, Arc<ChannelMetrics>),
}

impl<T> ChannelSender<T> {
    /// Wraps an externally-created bounded sender when the receiver is owned elsewhere.
    #[must_use]
    pub fn bounded(sender: mpsc::SyncSender<T>) -> ChannelSender<T> {
        ChannelSender::Bounded(sender, Arc::new(ChannelMetrics::default()))
    }

    /// Sends one value, blocking only when this is a full bounded channel.
    ///
    /// # Errors
    ///
    /// Returns the unsent value when the receiving half has been dropped.
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        let started = Instant::now();
        let metrics = match self {
            Self::Unbounded(_, metrics) | Self::Bounded(_, metrics) => metrics,
        };
        if let Self::Bounded(_, _) = self {
            metrics.mark_capacity_if_full();
        }
        match self {
            Self::Unbounded(sender, _) => sender.send(value),
            Self::Bounded(sender, _) => sender.send(value),
        }
        .map(|()| {
            metrics.sent.fetch_add(1, Ordering::Relaxed);
            let depth = metrics.depth.fetch_add(1, Ordering::Relaxed).saturating_add(1);
            update_peak(&metrics.peak_depth, depth);
            metrics.mark_capacity_if_full();
            let wait = started.elapsed();
            metrics.send_wait_ns.fetch_add(to_nanos(wait), Ordering::Relaxed);
            // Only a bounded channel can back-pressure a producer, so a slow send on an unbounded
            // channel (a busy consumer, a scheduler hiccup) is not counted as blocking.
            if metrics.capacity.is_some() && wait >= Duration::from_millis(1) {
                metrics.blocked_sends.fetch_add(1, Ordering::Relaxed);
            }
        })
        .inspect_err(|_| {
            metrics.disconnected.fetch_add(1, Ordering::Relaxed);
        })
    }

    /// Returns the shared metrics for this channel.
    #[must_use]
    pub fn metrics(&self) -> Arc<ChannelMetrics> {
        match self {
            Self::Unbounded(_, metrics) | Self::Bounded(_, metrics) => Arc::clone(metrics),
        }
    }
}

/// The receiving half of an instrumented channel.
pub struct ChannelReceiver<T> {
    receiver: mpsc::Receiver<T>,
    metrics: Arc<ChannelMetrics>,
}

impl<T> ChannelReceiver<T> {
    /// Receives one value while measuring consumer wait time and queue depth.
    ///
    /// # Errors
    /// Returns [`RecvError`] when every sender has disconnected.
    pub fn recv(&self) -> Result<T, RecvError> {
        let started = Instant::now();
        let result = self.receiver.recv();
        self.metrics.receive_wait_ns.fetch_add(to_nanos(started.elapsed()), Ordering::Relaxed);
        if result.is_ok() {
            self.metrics.received.fetch_add(1, Ordering::Relaxed);
            self.metrics.decrement_depth();
        } else {
            self.metrics.disconnected.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    /// Attempts to receive one value without blocking.
    ///
    /// # Errors
    /// Returns [`TryRecvError::Empty`] when no value is ready, or [`TryRecvError::Disconnected`]
    /// when every sender has disconnected.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        let result = self.receiver.try_recv();
        if result.is_ok() {
            self.metrics.received.fetch_add(1, Ordering::Relaxed);
            self.metrics.decrement_depth();
        }
        result
    }

    /// Returns a blocking iterator over every value until all senders disconnect.
    #[must_use]
    pub const fn iter(&self) -> ChannelRefIter<'_, T> {
        ChannelRefIter(self)
    }

    /// Returns the shared metrics for this channel.
    #[must_use]
    pub fn metrics(&self) -> Arc<ChannelMetrics> {
        Arc::clone(&self.metrics)
    }
}

impl<T> IntoIterator for ChannelReceiver<T> {
    type Item = T;
    type IntoIter = ChannelIntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        ChannelIntoIter(self)
    }
}

impl<'a, T> IntoIterator for &'a ChannelReceiver<T> {
    type Item = T;
    type IntoIter = ChannelRefIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        ChannelRefIter(self)
    }
}

/// Iterator over an owned channel receiver.
pub struct ChannelIntoIter<T>(ChannelReceiver<T>);

impl<T> Iterator for ChannelIntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.recv().ok()
    }
}

/// Iterator over a borrowed channel receiver.
pub struct ChannelRefIter<'a, T>(&'a ChannelReceiver<T>);

impl<T> Iterator for ChannelRefIter<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.recv().ok()
    }
}

/// Queue and wait measurements for one channel.
#[derive(Debug, SmartDefault)]
pub struct ChannelMetrics {
    /// Number of messages successfully sent.
    pub sent: AtomicU64,
    /// Number of messages successfully received.
    pub received: AtomicU64,
    /// Current number of messages waiting in the channel.
    pub depth: AtomicUsize,
    /// Largest observed queue depth.
    pub peak_depth: AtomicUsize,
    /// Aggregate sender wait time.
    pub send_wait_ns: AtomicU64,
    /// Aggregate receiver wait time.
    pub receive_wait_ns: AtomicU64,
    /// Sends that took at least one millisecond.
    pub blocked_sends: AtomicU64,
    pub(crate) disconnected: AtomicU64,
    pub(crate) capacity: Option<usize>,
    pub(crate) time_at_capacity_ns: AtomicU64,
    /// When the queue last reached capacity, as nanoseconds since [`ChannelMetrics::created`], or
    /// zero when it is not currently full. An atomic rather than a `Mutex<Option<Instant>>` because
    /// a bounded channel touches this on every send and every receive.
    pub(crate) capacity_since_ns: AtomicU64,
    /// The origin the capacity marker is measured from, so a full window is two `u64` subtractions
    /// rather than a stored `Instant` per marking.
    #[default(_code = "Instant::now()")]
    pub(crate) created: Instant,
    /// The stage boundary this queue sits on, so the run summary can name it. `None` for channels
    /// created without a boundary (internal handoffs and tests).
    pub(crate) boundary: Option<ChannelBoundary>,
}

/// Creates a channel with the requested throughput or back-pressure policy.
#[must_use]
pub fn channel<T>(policy: ChannelPolicy) -> (ChannelSender<T>, ChannelReceiver<T>) {
    channel_with_telemetry(policy, None, None)
}

/// Creates a channel and records its queue metrics in the run telemetry registry, tagged with the
/// stage boundary it sits on so the run summary can name it.
#[must_use]
pub fn channel_with_telemetry<T>(
    policy: ChannelPolicy,
    telemetry: Option<Arc<RunTelemetry>>,
    boundary: Option<ChannelBoundary>,
) -> (ChannelSender<T>, ChannelReceiver<T>) {
    let capacity = match policy {
        ChannelPolicy::Bounded(capacity) => Some(capacity.get()),
        ChannelPolicy::Unbounded => None,
    };
    let metrics = Arc::new(ChannelMetrics {
        capacity,
        boundary,
        ..ChannelMetrics::default()
    });
    if let Some(telemetry) = telemetry {
        telemetry.register_channel(Arc::clone(&metrics));
    }
    let sender = match policy {
        ChannelPolicy::Unbounded => {
            let (sender, receiver) = mpsc::channel();
            (ChannelSender::Unbounded(sender, Arc::clone(&metrics)), receiver)
        }
        ChannelPolicy::Bounded(capacity) => {
            let (sender, receiver) = mpsc::sync_channel(capacity.get());
            (ChannelSender::Bounded(sender, Arc::clone(&metrics)), receiver)
        }
    };
    let (sender, receiver) = sender;
    let receiver = ChannelReceiver {
        receiver,
        metrics: Arc::clone(&metrics),
    };
    (sender, receiver)
}

impl ChannelMetrics {
    /// Returns the current offset from the metrics origin, never zero.
    ///
    /// Zero is the "not currently at capacity" marker, so the first nanosecond of a run rounds up to
    /// one rather than reading as an unset marker.
    pub(crate) fn now_ns(&self) -> u64 {
        to_nanos(self.created.elapsed()).max(1)
    }

    fn mark_capacity_if_full(&self) {
        // The marker is read before the clock so a channel that is already known to be full (the
        // common case while a producer is blocked) costs one relaxed load and nothing else.
        if self.capacity.is_some_and(|capacity| self.depth.load(Ordering::Relaxed) >= capacity) && self.capacity_since_ns.load(Ordering::Relaxed) == 0 {
            let _ = self.capacity_since_ns.compare_exchange(0, self.now_ns(), Ordering::Relaxed, Ordering::Relaxed);
        }
    }

    fn decrement_depth(&self) {
        let previous = self
            .depth
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |depth| Some(depth.saturating_sub(1)))
            .unwrap_or(0);
        if self.capacity.is_some_and(|capacity| previous >= capacity) {
            let since = self.capacity_since_ns.swap(0, Ordering::Relaxed);
            if since != 0 {
                self.time_at_capacity_ns.fetch_add(self.now_ns().saturating_sub(since), Ordering::Relaxed);
            }
        }
    }
}

fn update_peak(peak: &AtomicUsize, value: usize) {
    let mut current = peak.load(Ordering::Relaxed);
    while value > current {
        match peak.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(next) => current = next,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ChannelPolicy, ChannelSender, channel};
    use std::{
        num::NonZeroUsize,
        sync::{
            atomic::Ordering,
            mpsc::{self, RecvTimeoutError, TryRecvError},
        },
        thread,
        time::Duration,
    };

    #[test]
    fn channel_metrics_track_queue_depth_and_waiting() {
        let (sender, receiver) = channel::<u8>(ChannelPolicy::Bounded(NonZeroUsize::new(2).unwrap()));
        sender.send(1).unwrap();
        sender.send(2).unwrap();
        assert_eq!(sender.metrics().depth.load(Ordering::Relaxed), 2);
        assert_eq!(receiver.recv().unwrap(), 1);
        assert_eq!(sender.metrics().depth.load(Ordering::Relaxed), 1);
        assert_eq!(sender.metrics().peak_depth.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn unbounded_channels_use_the_unbounded_sender() {
        let (sender, receiver) = channel(ChannelPolicy::Unbounded);
        assert!(matches!(sender, ChannelSender::Unbounded(_, _)));
        sender.send(7).unwrap();
        assert_eq!(receiver.recv().unwrap(), 7);
    }

    #[test]
    fn bounded_channels_use_the_requested_capacity() {
        let (sender, receiver) = channel(ChannelPolicy::Bounded(NonZeroUsize::new(1).unwrap()));
        assert!(matches!(sender, ChannelSender::Bounded(_, _)));
        sender.send(7).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), 7);
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn a_full_bounded_channel_back_pressures_its_producer() {
        let (sender, receiver) = channel(ChannelPolicy::Bounded(NonZeroUsize::new(1).unwrap()));
        sender.send(1).unwrap();
        let (finished_tx, finished_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            sender.send(2).unwrap();
            finished_tx.send(()).unwrap();
        });
        assert_eq!(finished_rx.recv_timeout(Duration::from_millis(25)), Err(RecvTimeoutError::Timeout));
        assert_eq!(receiver.recv().unwrap(), 1);
        finished_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(receiver.recv().unwrap(), 2);
        handle.join().unwrap();
    }

    #[test]
    fn a_receiver_stays_open_until_every_sender_clone_is_dropped() {
        let (sender, receiver) = channel::<u8>(ChannelPolicy::Unbounded);
        let clone = sender.clone();
        drop(sender);
        clone.send(7).unwrap();
        drop(clone);
        assert_eq!(receiver.recv().unwrap(), 7);
        assert!(receiver.recv().is_err());
    }
}
