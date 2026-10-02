use cassiopeia_mapping::template::error::TemplateError;
use std::num::NonZeroU64;
use urn_rs::Urn;

/// What one attribute lost to a template that could not be resolved, over one batch.
///
/// Only the first entity and the first failure are kept. A template that fails on one record
/// usually fails on every record carrying the same gap (a null column, a malformed cell), so keying
/// the record on the entity would grow it with the input and hand the reporter a line per record.
/// The attribute is the failure; the entity is one place to go and look, and the count says how far
/// it spread.
#[derive(Debug)]
pub struct UnresolvedTemplate {
    /// The first entity that lost the attribute, kept so a mapping author can find a record that
    /// shows the failure.
    pub entity: Urn,
    /// The first failure, kept whole so a report can walk its cause chain down to the templating
    /// engine's own reason.
    pub error: TemplateError,
    /// How many entities lost the attribute.
    pub occurrences: NonZeroU64,
}

impl UnresolvedTemplate {
    /// Opens a record for one attribute, holding `entity` and `error` as its first failure.
    #[must_use]
    pub fn first(entity: &Urn, error: TemplateError) -> UnresolvedTemplate {
        UnresolvedTemplate {
            // The entity keeps its own identifier, and the record outlives it, so the identifier is
            // cloned; this runs once per attribute and batch, on the failure path only.
            entity: entity.clone(),
            error,
            occurrences: NonZeroU64::MIN,
        }
    }

    /// Counts one more entity, keeping the first failure already held.
    pub const fn count_another(&mut self) {
        self.occurrences = self.occurrences.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use crate::unresolved_template::UnresolvedTemplate;
    use cassiopeia_mapping::template::{error::TemplateError, template_name::TemplateName};
    use urn_rs::Urn;

    fn failure(source: &str) -> TemplateError {
        TemplateError::Decode {
            template: TemplateName::for_source(source),
            source: serde_json::from_str::<serde_json::Value>("[").unwrap_err(),
        }
    }

    #[test]
    fn the_first_entity_and_failure_are_kept_however_many_follow() {
        let first: Urn = "urn:ngsi-ld:Country:Bonaire".parse().unwrap();
        let mut record = UnresolvedTemplate::first(&first, failure("{{ a | split(pat=' ') }}"));
        record.count_another();
        record.count_another();

        assert_eq!(record.entity, first);
        assert!(matches!(&record.error, TemplateError::Decode { template, .. } if *template == TemplateName::for_source("{{ a | split(pat=' ') }}")));
        assert_eq!(record.occurrences.get(), 3);
    }
}
