use cassiopeia_common::representation::NgsiLdRepresentation;
use cassiopeia_manifest::output::temporal::TemporalRepresentation;
use serde_json::{Value, json};
use std::path::Path;

/// Which observations a golden run writes.
#[derive(Clone, Copy, Debug, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum History {
    /// The current state: per entity and mapping, the record with the latest `observedAt`.
    Current,
    /// Every observation of an entity, folded into one `EntityTemporal` (ETSI GS CIM 009 v1.9.1
    /// clause 5.2.20).
    Series,
}

/// The representations every history is written in.
pub(crate) const REPRESENTATIONS: [NgsiLdRepresentation; 3] = [
    NgsiLdRepresentation::Normalized,
    NgsiLdRepresentation::Concise,
    NgsiLdRepresentation::Simplified,
];

/// What one golden run writes: a history in one representation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct View {
    /// Which observations it writes.
    pub(crate) history: History,
    /// The representation it writes them in.
    pub(crate) representation: NgsiLdRepresentation,
}

impl View {
    /// The manifest `output` member that writes this view as files into `directory`.
    pub(crate) fn output(self, directory: &Path) -> Value {
        let mut output = json!({
            "target": "file",
            "directory": directory,
            "representation": self.representation,
        });
        match self.history {
            History::Current => {}
            History::Series => output["temporal"] = json!({ "representation": TemporalRepresentation::Series }),
        }
        output
    }

    /// The name of the snapshot holding the entities this view writes, such as `current-concise`.
    pub(crate) fn snapshot_name(self) -> String {
        let representation = json!(self.representation);
        format!("{}-{}", self.history, representation.as_str().unwrap())
    }
}
