use crate::unresolved_template::UnresolvedTemplate;
use cassiopeia_mapping::template::error::TemplateError;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use indexmap::IndexMap;
use parking_lot::Mutex;
use urn_rs::Urn;

/// The attributes one batch dropped because their template could not be resolved, one record per
/// attribute.
///
/// A template that fails to render for one record costs that record the attribute, not the whole
/// entity: one optional attribute's gap must not take down everything else the record carries. The
/// loss is still named, here, so the run can say which attribute it dropped and where.
///
/// The sink is shared, not owned per entity: a batch extracts across a Rayon pool, so every worker
/// records into the same one behind a shared reference. The map is an [`IndexMap`] behind a
/// [`Mutex`] rather than a `DashMap` so a run's messages come out in the order the failures were met,
/// and because a sink stays empty in almost every batch; the lock is only taken on the failure path.
#[derive(Default)]
pub struct UnresolvedTemplates {
    entries: Mutex<IndexMap<NameBuf, UnresolvedTemplate>>,
}

impl UnresolvedTemplates {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> UnresolvedTemplates {
        UnresolvedTemplates::default()
    }

    /// Records that `entity` lost `attribute` because its template failed with `error`.
    ///
    /// The first failure an attribute meets is kept as its example; every later one is counted
    /// against that same record. Which one is first, when a batch is extracted in parallel, is
    /// whichever reached the sink first: the example points at a record, it does not rank them.
    pub fn record(&self, attribute: &NameBuf, entity: &Urn, error: TemplateError) {
        self.entries
            .lock()
            .entry(attribute.clone())
            .and_modify(UnresolvedTemplate::count_another)
            .or_insert_with(|| UnresolvedTemplate::first(entity, error));
    }

    /// Whether every template resolved, which is the common case for a batch.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    /// Every attribute that was dropped and what dropped it, in the order they first failed.
    #[must_use]
    pub fn into_entries(self) -> Vec<(NameBuf, UnresolvedTemplate)> {
        self.entries.into_inner().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::unresolved_templates::UnresolvedTemplates;
    use cassiopeia_mapping::template::{error::TemplateError, template_name::TemplateName};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use urn_rs::Urn;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    fn failure() -> TemplateError {
        TemplateError::Decode {
            template: TemplateName::for_source("{{ codes | split(pat=' ') }}"),
            source: serde_json::from_str::<serde_json::Value>("[").unwrap_err(),
        }
    }

    #[test]
    fn a_fresh_sink_holds_nothing() {
        assert!(UnresolvedTemplates::new().is_empty());
    }

    #[test]
    fn every_entity_one_attribute_fails_on_is_counted_against_one_record() {
        let sink = UnresolvedTemplates::new();
        sink.record(&name("dafifCode"), &urn("urn:ngsi-ld:Country:A"), failure());
        sink.record(&name("dafifCode"), &urn("urn:ngsi-ld:Country:B"), failure());

        let entries = sink.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.entity, urn("urn:ngsi-ld:Country:A"));
        assert_eq!(entries[0].1.occurrences.get(), 2);
    }

    #[test]
    fn two_attributes_are_two_records_in_the_order_they_failed() {
        let sink = UnresolvedTemplates::new();
        sink.record(&name("dafifCode"), &urn("urn:ngsi-ld:Country:A"), failure());
        sink.record(&name("isoCode"), &urn("urn:ngsi-ld:Country:A"), failure());

        let entries = sink.into_entries();
        assert_eq!(entries[0].0, name("dafifCode"));
        assert_eq!(entries[1].0, name("isoCode"));
    }
}
