use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// The delivery guarantee a broker writer offers, orthogonal to which operation it runs.
///
/// [`Streaming`](BrokerAtomicity::Streaming) pushes entities to the broker as they arrive.
/// [`Atomic`](BrokerAtomicity::Atomic) spools every entity to a temporary file first and pushes
/// nothing until the whole pipeline finishes cleanly, so a mid-pipeline failure never leaves a
/// half-populated broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum BrokerAtomicity {
    /// Push entities to the broker as they stream through the writer.
    #[default]
    Streaming,
    /// Spool every entity and push only after the pipeline finishes cleanly.
    Atomic,
}

impl BrokerAtomicity {
    /// Bridges the CLI's boolean `--atomic` toggle onto the atomicity axis.
    #[must_use]
    pub const fn from_flag(atomic: bool) -> BrokerAtomicity {
        if atomic { BrokerAtomicity::Atomic } else { BrokerAtomicity::Streaming }
    }
}

#[cfg(test)]
mod tests {
    use crate::broker_atomicity::BrokerAtomicity;

    #[test]
    fn every_atomicity_round_trips_through_its_kebab_case_token() {
        for (atomicity, token) in [(BrokerAtomicity::Streaming, r#""streaming""#), (BrokerAtomicity::Atomic, r#""atomic""#)] {
            assert_eq!(serde_json::to_string(&atomicity).unwrap(), token);
            assert_eq!(serde_json::from_str::<BrokerAtomicity>(token).unwrap(), atomicity);
        }
    }

    #[test]
    fn atomicity_defaults_to_streaming() {
        assert_eq!(BrokerAtomicity::default(), BrokerAtomicity::Streaming);
    }

    #[test]
    fn the_atomic_flag_maps_onto_the_atomic_variant() {
        assert_eq!(BrokerAtomicity::from_flag(true), BrokerAtomicity::Atomic);
        assert_eq!(BrokerAtomicity::from_flag(false), BrokerAtomicity::Streaming);
    }
}
