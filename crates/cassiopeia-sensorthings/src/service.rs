use crate::{service_root::ServiceRoot, settings::sensorthings_settings::SensorThingsSettings};
use getset::Getters;

/// A SensorThings API server a remote input reads, and how the input reads it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Getters)]
#[getset(get = "pub")]
pub struct SensorThingsService {
    /// The server's service root, which every request is built on.
    root: ServiceRoot,
    /// How the server is read.
    settings: SensorThingsSettings,
}

impl SensorThingsService {
    /// The server at `root`, read as `settings` say.
    #[must_use]
    pub const fn new(root: ServiceRoot, settings: SensorThingsSettings) -> SensorThingsService {
        SensorThingsService { root, settings }
    }

    /// Consumes the service and returns its service root.
    #[must_use]
    pub fn into_root(self) -> ServiceRoot {
        self.root
    }

    /// Consumes the service and returns how it is read.
    #[must_use]
    pub fn into_settings(self) -> SensorThingsSettings {
        self.settings
    }
}
