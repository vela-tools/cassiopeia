//! Scheduling stage for the Cassiopeia pipeline.
//!
//! The scheduler decides *when* the rest of the pipeline runs. It consumes the manifest's typed
//! [`Schedule`](cassiopeia_manifest::schedule::Schedule) (a manifest without one runs its inputs
//! exactly once) and drives a background thread that emits one
//! [`SchedulerPayload`](payload::SchedulerPayload) per run over a rendezvous channel, so the sender
//! blocks until the pipeline has consumed a run before it computes the wait for the next one.
//!
//! Every value that could be malformed (an interval, a cron expression, a time of day) is already
//! parsed and validated at the manifest boundary, so the running stage cannot fail: its channel
//! carries [`Infallible`](std::convert::Infallible) as its error type.

mod announcement;
pub mod payload;
pub mod sleep;
pub mod stage;
pub mod wait;
