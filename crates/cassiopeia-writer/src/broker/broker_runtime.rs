use cassiopeia_reporter::{reporter::Reporter, stage_id::StageId};
use std::sync::atomic::AtomicBool;

/// The process-lifetime runtime hooks a broker writer's workers observe.
pub struct BrokerRuntime {
    /// The reporter stage id for the live latency readout; `None` suppresses it.
    pub stage_id: Option<StageId>,
    /// The process-wide shutdown flag the workers observe.
    pub shutdown: &'static AtomicBool,
    /// The reporter warnings and errors are sent to.
    pub reporter: &'static dyn Reporter,
}

impl BrokerRuntime {
    /// Builds the runtime hooks from the shutdown flag and reporter, with no stage readout.
    #[must_use]
    pub const fn new(shutdown: &'static AtomicBool, reporter: &'static dyn Reporter) -> BrokerRuntime {
        BrokerRuntime {
            stage_id: None,
            shutdown,
            reporter,
        }
    }
}
