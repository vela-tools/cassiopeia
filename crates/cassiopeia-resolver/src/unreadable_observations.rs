use crate::{entity_store::discarded_timestamps::UnreadableDiscards, mapping_id::MappingId};
use ahash::AHashMap;
use cassiopeia_mapping::{
    mapping::Mapping,
    observed_at::ObservedAt,
    observed_declaration::ObservedDeclaration,
    observed_template::{ObservedTemplate, ObservedTemplateIndex},
};
use std::{num::NonZeroU64, sync::Arc};

/// What unreadable text under one `observedAt` template of one mapping cost across a scan: the
/// records a current-state store released because their text read as no instant and other records
/// of the same id held every ranking.
///
/// Only the first text is kept, as for any timestamp that will not read: the spelling is the
/// problem, the text is one example of it, and the count says how far it spread.
#[derive(Debug)]
pub struct UnreadableObservation {
    /// The mapping whose records were released, which names the declarations reading the text.
    mapping: Arc<Mapping>,
    /// The template that rendered the text.
    template: ObservedTemplateIndex,
    /// The first text that read as no instant.
    pub example: ObservedAt,
    /// How many records were discarded with such text, across every id.
    pub records: NonZeroU64,
}

impl UnreadableObservation {
    /// Every `observedAt` sub-attribute that read the text, each of which the discarded records
    /// would have cost had they been kept.
    #[must_use]
    pub fn declarations(&self) -> &[ObservedDeclaration] {
        self.mapping
            .observation_plan()
            .template(self.template)
            .map_or(&[], ObservedTemplate::declarations)
    }
}

/// The records an assembly scan found released for unreadable `observedAt` text, one entry per
/// mapping and template.
#[derive(Debug, Default)]
pub struct UnreadableObservations {
    by_template: AHashMap<(MappingId, ObservedTemplateIndex), UnreadableObservation>,
}

impl UnreadableObservations {
    /// Opens an empty tally.
    #[must_use]
    pub fn new() -> UnreadableObservations {
        UnreadableObservations::default()
    }

    /// Records the records of `mapping`, interned as `mapping_id`, that a slot released for
    /// unreadable `observedAt` text.
    pub fn record(&mut self, mapping_id: MappingId, mapping: &Arc<Mapping>, discarded: &UnreadableDiscards) {
        let key = (mapping_id, discarded.template);
        if let Some(unreadable) = self.by_template.get_mut(&key) {
            unreadable.records = unreadable.records.saturating_add(discarded.records.get());
            return;
        }
        self.by_template.insert(
            key,
            UnreadableObservation {
                mapping: Arc::clone(mapping),
                template: discarded.template,
                // The tally outlives the slot the text was read off, which the scan drops with its
                // entity; only a mapping's first discard pays for the copy.
                example: discarded.example.clone(),
                records: discarded.records,
            },
        );
    }

    /// Whether the scan found no records released for unreadable text.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_template.is_empty()
    }

    /// Every mapping's releases, ordered by mapping id and then template so a report lists them the
    /// same way every run.
    #[must_use]
    pub fn into_entries(self) -> Vec<UnreadableObservation> {
        let mut entries: Vec<((MappingId, ObservedTemplateIndex), UnreadableObservation)> = self.by_template.into_iter().collect();
        entries.sort_by_key(|(key, _)| *key);
        entries.into_iter().map(|(_, unreadable)| unreadable).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_store::discarded_timestamps::UnreadableDiscards, mapping_id::MappingId, unreadable_observations::UnreadableObservations};
    use cassiopeia_mapping::{mapping::Mapping, observed_at::ObservedAt, observed_template::ObservedTemplateIndex, template::runner::TemplateRunner};
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU64, path::Path, sync::Arc};

    fn mapping(time: &str) -> Arc<Mapping> {
        let document = format!(
            r#"{{
                version: "v4",
                dataModel: "Sensor",
                identity: {{ entityName: "{{{{ id }}}}" }},
                attributes: {{ reading: {{ source: "{{{{ r }}}}", properties: {{ observedAt: {{ source: "{{{{ {time} }}}}" }} }} }} }},
            }}"#
        );
        Arc::new(Mapping::from_json5(&document, Path::new("sensor.json5"), &mut TemplateRunner::new()).unwrap())
    }

    fn discards(text: &str, records: u64) -> UnreadableDiscards {
        UnreadableDiscards {
            template: ObservedTemplateIndex::new(0),
            example: ObservedAt::new(text),
            records: NonZeroU64::new(records).unwrap(),
        }
    }

    #[test]
    fn a_fresh_tally_holds_nothing() {
        assert!(UnreadableObservations::new().is_empty());
        assert!(UnreadableObservations::new().into_entries().is_empty());
    }

    #[test]
    fn one_mapping_keeps_its_first_text_and_sums_its_records() {
        let sensor = mapping("time");
        let mut tally = UnreadableObservations::new();

        tally.record(MappingId::new(1), &sensor, &discards("soon", 2));
        tally.record(MappingId::new(1), &sensor, &discards("later", 3));

        let entries = tally.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!((entries[0].example.as_str(), entries[0].records.get()), ("soon", 5));
        assert_eq!(entries[0].declarations().len(), 1);
    }

    #[test]
    fn each_mapping_is_its_own_entry_in_mapping_order() {
        let mut tally = UnreadableObservations::new();

        tally.record(MappingId::new(2), &mapping("b"), &discards("second", 1));
        tally.record(MappingId::new(1), &mapping("a"), &discards("first", 1));

        let examples: Vec<String> = tally.into_entries().into_iter().map(|unreadable| unreadable.example.to_string()).collect();
        assert_eq!(examples, ["first", "second"]);
    }
}
