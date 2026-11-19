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
    /// The entity counted last under this pair. A series run checks each record of an entity on its
    /// own, and the records of one entity arrive in a row, so a record of the entity counted last
    /// adds nothing to the count.
    latest: Urn,
    /// How many entities were warned about.
    occurrences: NonZeroU64,
}

/// Whether a warned record counted a new entity under its reason and type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WarnedEntity {
    /// The record is the first warned about of its entity, which now counts as one more warning.
    New,
    /// The record belongs to the entity warned about last, which already counted.
    Repeated,
}

/// The entity types a run warned about, grouped by why.
///
/// Relaxed validation deliberately asks for no per-entity diagnostics, so there is nothing to report
/// beyond the type and the reason, which is exactly the grouping a reader wants anyway. Each group
/// counts entities, not records: a series run checks each record of an entity on its own, and the
/// count names how many of the entities the run writes were warned about. A repeat costs one hash
/// probe and one comparison, and only a new entity clones an identifier.
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

    /// Records one warned record under its reason and type, counting its entity unless the record
    /// belongs to the entity counted last there. The records of one entity arrive in a row, as the
    /// store emits them, so each entity counts once.
    pub(crate) fn record(&mut self, reason: WarnReason, entity: &NgsiLdEntity) -> WarnedEntity {
        if let Some(warned) = self.entries.get_mut(&(reason, entity.entity_type.clone())) {
            if warned.latest == entity.id {
                return WarnedEntity::Repeated;
            }
            // The entry outlives the borrowed entity, so it keeps its own copy of the id.
            warned.latest = entity.id.clone();
            warned.occurrences = warned.occurrences.saturating_add(1);
            return WarnedEntity::New;
        }

        // A pair seen for the first time owns its type name as the map key, and the exemplar and the
        // latest id must outlive the borrowed entity.
        self.entries.insert(
            (reason, entity.entity_type.clone()),
            WarnedType {
                exemplar: entity.id.clone(),
                latest: entity.id.clone(),
                occurrences: NonZeroU64::MIN,
            },
        );
        WarnedEntity::New
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
    use crate::stages::{
        nonconformant_types::{NonconformantTypes, WarnedEntity},
        validation_decision::WarnReason,
    };
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use cassiopeia_reporter::backend::noop::NoopReporter;
    use pretty_assertions::assert_eq;
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
    fn the_records_of_one_entity_in_a_row_count_as_one_entity() {
        let mut sink = NonconformantTypes::new();
        let recorded: Vec<WarnedEntity> = ["1", "1", "1", "2", "2"]
            .iter()
            .map(|id| {
                sink.record(
                    WarnReason::SchemaAbsent,
                    &entity("TropicalCyclone", &format!("urn:ngsi-ld:TropicalCyclone:{id}")),
                )
            })
            .collect();

        assert_eq!(
            recorded,
            [
                WarnedEntity::New,
                WarnedEntity::Repeated,
                WarnedEntity::Repeated,
                WarnedEntity::New,
                WarnedEntity::Repeated
            ]
        );
        let warned = sink.entries.values().next().expect("a group");
        assert_eq!(warned.occurrences.get(), 2);
        assert_eq!(warned.exemplar.to_string(), "urn:ngsi-ld:TropicalCyclone:1");
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
