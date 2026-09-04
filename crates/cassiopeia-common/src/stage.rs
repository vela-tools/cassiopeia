//! The single typed identity every pipeline measurement and progress key shares.

use strum::{Display, EnumString, IntoStaticStr};

/// A pipeline stage, in declaration = execution order.
///
/// One typed identity is the single source of truth for stage names: telemetry, the bottleneck
/// classifier, and the terminal reporter all key off it rather than each encoding a raw string of
/// its own. The derived kebab-case string (`"resolver"`, `"extractor"`, ...) is the stable id used
/// as both the reporter and telemetry key, while
/// [`Stage::label`] owns the human-facing display name. The `Ord` derive follows declaration order,
/// so a `BTreeMap<Stage, _>` iterates the stages in pipeline order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display, EnumString, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum Stage {
    /// Reads sources into collected payloads.
    Collector,
    /// Detects each payload's format.
    Profiler,
    /// Parses payloads into records.
    Ingestor,
    /// Turns records into mapped fragments.
    Expander,
    /// Merges fragments into the entity store (phase 1).
    Resolver,
    /// Scans the store and assembles each id's stored fragments into entities (phase 2 source).
    Assembler,
    /// Resolves each assembled entity's attribute values from its source data.
    Extractor,
    /// Builds NGSI-LD entities.
    Transformer,
    /// Checks entities against their schemas.
    Validator,
    /// Folds each id's single-instance observations into one temporal entity (ETSI GS CIM 009
    /// v1.9.1 clause 5.2.20). Runs only for a series-representation run, between the validator and
    /// the writer.
    Aggregator,
    /// Emits entities to the destination.
    Writer,
}

impl Stage {
    /// Returns the stable string identity, the token used as the reporter and telemetry key.
    #[must_use]
    pub fn id(self) -> &'static str {
        self.into()
    }

    /// Returns the human-readable label shown in progress bars and the run summary.
    ///
    /// The resolver's work splits across three stages: [`Stage::Resolver`] sinks fragments into the
    /// store (phase 1), [`Stage::Assembler`] scans the store and assembles each id's fragments into
    /// entities (phase 2), and [`Stage::Extractor`] resolves each assembled entity's attribute values.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Collector => "Collector",
            Stage::Profiler => "Profiler",
            Stage::Ingestor => "Ingestor",
            Stage::Expander => "Expander",
            Stage::Resolver => "Resolver",
            Stage::Assembler => "Assembler",
            Stage::Extractor => "Extractor",
            Stage::Transformer => "Transformer",
            Stage::Validator => "Validator",
            Stage::Aggregator => "Aggregator",
            Stage::Writer => "Writer",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::stage::Stage;
    use std::str::FromStr;

    #[test]
    fn each_stage_round_trips_through_its_kebab_case_id() {
        for stage in [
            Stage::Collector,
            Stage::Profiler,
            Stage::Ingestor,
            Stage::Expander,
            Stage::Resolver,
            Stage::Assembler,
            Stage::Extractor,
            Stage::Transformer,
            Stage::Validator,
            Stage::Aggregator,
            Stage::Writer,
        ] {
            assert_eq!(Stage::from_str(stage.id()), Ok(stage));
        }
    }

    #[test]
    fn the_resolver_assembler_and_extractor_carry_distinct_ids_and_labels() {
        assert_eq!(Stage::Resolver.id(), "resolver");
        assert_eq!(Stage::Assembler.id(), "assembler");
        assert_eq!(Stage::Extractor.id(), "extractor");
        assert_eq!(Stage::Resolver.label(), "Resolver");
        assert_eq!(Stage::Assembler.label(), "Assembler");
        assert_eq!(Stage::Extractor.label(), "Extractor");
    }

    #[test]
    fn the_id_matches_the_display_string() {
        assert_eq!(Stage::Writer.id(), "writer");
        assert_eq!(Stage::Writer.to_string(), "writer");
    }
}
