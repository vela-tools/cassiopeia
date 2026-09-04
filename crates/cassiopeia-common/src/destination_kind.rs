//! The command-line vocabulary for where a run's output goes.
//!
//! Two words are used deliberately and kept distinct throughout the codebase: a *writer* is the
//! component that emits entities, and a *destination* is where they go. [`DestinationKind`] is the
//! flat command-line discriminant a user selects with `--writer`; it names which kind of
//! destination the run builds. The manifest's own `Destination` type is the structured form that
//! carries each kind's own settings.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Which kind of destination a run's writer sends its entities to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum DestinationKind {
    /// Write entities to the file system.
    File,
    /// Send entities to an NGSI-LD context broker.
    ContextBroker,
}

#[cfg(test)]
mod tests {
    use crate::destination_kind::DestinationKind;

    #[test]
    fn the_file_kind_uses_a_kebab_case_wire_form() {
        assert_eq!(serde_json::to_string(&DestinationKind::File).unwrap(), r#""file""#);
        assert_eq!(serde_json::from_str::<DestinationKind>(r#""file""#).unwrap(), DestinationKind::File);
    }

    #[test]
    fn the_broker_kind_uses_a_kebab_case_wire_form() {
        assert_eq!(serde_json::to_string(&DestinationKind::ContextBroker).unwrap(), r#""context-broker""#);
        assert_eq!(
            serde_json::from_str::<DestinationKind>(r#""context-broker""#).unwrap(),
            DestinationKind::ContextBroker
        );
    }
}
