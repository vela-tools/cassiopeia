//! The one rule every HTTP request Cassiopeia sends follows when its answer says to try again later:
//! which answers are transient, how long to wait before the next attempt, and how to wait so that
//! shutdown still ends the wait. A source the collector fetches and a Context Broker the writer
//! posts to are retried alike.

pub mod backoff;
pub mod retry_after;
pub mod transient_status;
