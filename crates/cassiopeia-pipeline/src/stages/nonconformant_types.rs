use crate::stages::validation_decision::WarnReason;
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, schema_code::SchemaCode},
    context_field::{ContextField, minted_id},
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
use cassiopeia_reporter::reporter::DiagnosticSink;
use indexmap::IndexMap;
use std::num::NonZeroU64;
use urn_rs::Urn;

/// What one warned (reason, type) pair has accounted for.
struct WarnedType {
    /// The first entity warned about under this pair, so the report can point at a real offender.
    exemplar: Urn,
    /// How many entities were warned about.
    occurrences: NonZeroU64,
}

/// The entity types a run warned about, grouped by why.
///
/// Relaxed validation deliberately asks for no per-entity diagnostics, so there is nothing to report
/// beyond the type and the reason, which is exactly the grouping a reader wants anyway. A repeat
/// costs one hash probe and one increment, and only a pair never seen before clones an identifier.
///
/// The map is an [`IndexMap`] so a run over the same source reports its types in the same order
/// every time.
#[derive(Default)]
pub(crate) struct NonconformantTypes {
    entries: IndexMap<(WarnReason, NameBuf), WarnedType>,
}

impl NonconformantTypes {
    /// Opens an empty sink.
    pub(crate) fn new() -> NonconformantTypes {
        NonconformantTypes::default()
    }

    /// Records one warned entity under its reason and type.
    pub(crate) fn record(&mut self, reason: WarnReason, entity: &NgsiLdEntity) {
        if let Some(warned) = self.entries.get_mut(&(reason, entity.entity_type.clone())) {
            warned.occurrences = warned.occurrences.saturating_add(1);
            return;
        }

        // A pair seen for the first time is the only case that owns anything: the map key must own
        // the type name and the exemplar must outlive the borrowed entity.
        self.entries.insert(
            (reason, entity.entity_type.clone()),
            WarnedType {
                exemplar: entity.id.clone(),
                occurrences: NonZeroU64::MIN,
            },
        );
    }

    /// Whether nothing was warned about.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Reports one diagnostic per (reason, type), each naming an exemplar and carrying its count.
    pub(crate) fn report(self, sink: &dyn DiagnosticSink) {
        for ((reason, entity_type), warned) in self.entries {
            let count = warned.occurrences.get();
            sink.report(
                &DiagnosticBuilder::new(
                    Severity::Warning,
                    DiagnosticCode::Schema(code_of(reason)),
                    headline(reason, count, &entity_type, &warned.exemplar),
                )
                .with_context(ContextField::EntityType(entity_type))
                .with_optional_context(minted_id(&warned.exemplar).map(|first| ContextField::Entities {
                    first,
                    additional: count.saturating_sub(1),
                }))
                .with_occurrences(warned.occurrences)
                .build(),
            );
        }
    }
}

/// The code one warning reason is published under.
const fn code_of(reason: WarnReason) -> SchemaCode {
    match reason {
        WarnReason::SchemaAbsent => SchemaCode::Absent,
        WarnReason::Nonconformant => SchemaCode::Nonconformant,
        WarnReason::SchemaUnusable => SchemaCode::Unusable,
    }
}

/// The sentence one warned group renders as.
fn headline(reason: WarnReason, count: u64, entity_type: &NameBuf, exemplar: &Urn) -> String {
    match reason {
        WarnReason::SchemaAbsent => format!("{count} '{entity_type}' entities have no schema and were not validated (first: {exemplar})"),
        WarnReason::Nonconformant => format!("{count} '{entity_type}' entities do not conform to their schema (first: {exemplar})"),
        WarnReason::SchemaUnusable => format!("{count} '{entity_type}' entities could not be validated: their schema is unusable (first: {exemplar})"),
    }
}

#[cfg(test)]
mod tests {
    use crate::stages::{nonconformant_types::NonconformantTypes, validation_decision::WarnReason};
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use cassiopeia_reporter::backend::noop::NoopReporter;
    use urn_rs::Urn;

    fn entity(entity_type: &str, id: &str) -> NgsiLdEntity {
        NgsiLdEntity::new(id.parse::<Urn>().unwrap(), NameBuf::new(entity_type).unwrap())
    }

    #[test]
    fn a_fresh_sink_holds_nothing() {
        assert!(NonconformantTypes::new().is_empty());
    }

    #[test]
    fn entities_of_one_type_and_reason_collapse_into_one_group() {
        let mut sink = NonconformantTypes::new();
        for index in 0..1000 {
            sink.record(
                WarnReason::Nonconformant,
                &entity("AirQualityObserved", &format!("urn:ngsi-ld:AirQualityObserved:ES-{index}")),
            );
        }

        assert_eq!(sink.entries.len(), 1);
        let warned = sink.entries.values().next().expect("a group");
        assert_eq!(warned.occurrences.get(), 1000);
        assert_eq!(warned.exemplar.to_string(), "urn:ngsi-ld:AirQualityObserved:ES-0");
    }

    #[test]
    fn a_different_reason_or_type_is_a_different_group() {
        let mut sink = NonconformantTypes::new();
        sink.record(WarnReason::Nonconformant, &entity("A", "urn:ngsi-ld:A:1"));
        sink.record(WarnReason::SchemaAbsent, &entity("A", "urn:ngsi-ld:A:2"));
        sink.record(WarnReason::Nonconformant, &entity("B", "urn:ngsi-ld:B:1"));

        assert_eq!(sink.entries.len(), 3);
    }

    #[test]
    fn reporting_an_empty_sink_says_nothing() {
        NonconformantTypes::new().report(&NoopReporter::new());
    }
}
