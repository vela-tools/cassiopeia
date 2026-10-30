use crate::mapping_id::MappingId;
use ahash::RandomState;
use cassiopeia_mapping::template::error::TemplateError;
use dashmap::DashMap;

/// The first failure of each mapping's `observedAt` template, kept while fragments are stored.
///
/// A record whose template fails is stored ranked as having no timestamp, and a current-state store
/// may discard it for another record of the same id. The discard is only counted by the store, which
/// has no use for the error, so the error is kept here, one per mapping, for the report that names
/// the discarded records once assembly has counted them. A template that fails on one record usually
/// fails on every record carrying the same gap, so the first failure stands for them all.
#[derive(Debug, Default)]
pub struct ObservedAtFailures(DashMap<MappingId, TemplateError, RandomState>);

impl ObservedAtFailures {
    /// Opens an empty registry.
    #[must_use]
    pub fn new() -> ObservedAtFailures {
        ObservedAtFailures::default()
    }

    /// Records that the template of the mapping `mapping_id` failed with `error`, keeping the first
    /// failure a mapping meets.
    pub fn record(&self, mapping_id: MappingId, error: TemplateError) {
        self.0.entry(mapping_id).or_insert(error);
    }

    /// Takes the first failure of the mapping `mapping_id` out of the registry.
    #[must_use]
    pub fn take(&self, mapping_id: MappingId) -> Option<TemplateError> {
        self.0.remove(&mapping_id).map(|(_, error)| error)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::{mapping_id::MappingId, observed_at_failures::ObservedAtFailures};
    use cassiopeia_mapping::{
        template::{
            TemplateSource,
            error::{ResolutionFailure, TemplateError},
        },
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use std::{path::Path, sync::Arc};

    /// A failure of the `observedAt` template `source` in `thing.json5`.
    pub(crate) fn failure(source: &str) -> TemplateError {
        TemplateError {
            location: TemplateLocation::new(Arc::from(Path::new("thing.json5")), TemplateSite::ObservedAt),
            failure: Box::new(ResolutionFailure::Decode {
                template: TemplateSource::new(source),
                source: serde_json::from_str::<serde_json::Value>("[").unwrap_err(),
            }),
        }
    }

    /// What the failure says went wrong, which names the template as written.
    fn reason(error: &TemplateError) -> String {
        error.failure.to_string()
    }

    #[test]
    fn the_first_failure_of_a_mapping_is_kept_and_taken_once() {
        let failures = ObservedAtFailures::new();
        failures.record(MappingId::new(1), failure("{{ first }}"));
        failures.record(MappingId::new(1), failure("{{ second }}"));

        let kept = failures.take(MappingId::new(1)).map(|error| reason(&error)).unwrap_or_default();
        assert!(kept.contains("{{ first }}"), "{kept}");
        assert!(failures.take(MappingId::new(1)).is_none());
    }

    #[test]
    fn each_mapping_keeps_its_own_failure() {
        let failures = ObservedAtFailures::new();
        failures.record(MappingId::new(1), failure("{{ a }}"));
        failures.record(MappingId::new(2), failure("{{ b }}"));

        let kept = failures.take(MappingId::new(2)).map(|error| reason(&error)).unwrap_or_default();
        assert!(kept.contains("{{ b }}"), "{kept}");
        assert!(failures.take(MappingId::new(3)).is_none());
    }
}
