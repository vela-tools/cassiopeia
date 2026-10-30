use crate::unresolved_template::UnresolvedTemplate;
use cassiopeia_batch_sink::batch_sink::BatchSink;
use cassiopeia_mapping::template::error::TemplateError;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use derive_more::{Deref, Into};
use urn_rs::Urn;

/// The attributes one batch dropped because their template could not be resolved, one record per
/// attribute.
///
/// A template that fails to render for one record costs that record the attribute, not the whole
/// entity: one optional attribute's gap must not take down everything else the record carries. The
/// loss is still named, here, so the run can say which attribute it dropped and where.
#[derive(Default, Deref, Into)]
pub struct UnresolvedTemplates(BatchSink<NameBuf, UnresolvedTemplate>);

impl UnresolvedTemplates {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> UnresolvedTemplates {
        UnresolvedTemplates::default()
    }

    /// Records that `entity` lost `attribute` because its template failed with `error`.
    ///
    /// The first failure an attribute meets is kept as its example; every later one is counted
    /// against that same record.
    pub fn record(&self, attribute: &NameBuf, entity: &Urn, error: TemplateError) {
        // An attribute seen for the first time is the only case that owns anything: the map key
        // must own the attribute name, which is only borrowed from the mapping.
        self.0.tally(attribute, || (attribute.clone(), UnresolvedTemplate::first(entity, error)));
    }
}

#[cfg(test)]
mod tests {
    use crate::{unresolved_template::UnresolvedTemplate, unresolved_templates::UnresolvedTemplates};
    use cassiopeia_batch_sink::batch_sink::BatchSink;
    use cassiopeia_mapping::{
        template::{
            TemplateSource,
            error::{ResolutionFailure, TemplateError},
        },
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;
    use std::{path::Path, sync::Arc};
    use urn_rs::Urn;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    fn entries(sink: UnresolvedTemplates) -> Vec<(NameBuf, UnresolvedTemplate)> {
        BatchSink::from(sink).into_entries().collect()
    }

    fn failure() -> TemplateError {
        TemplateError {
            location: TemplateLocation::new(Arc::from(Path::new("sensor.json5")), TemplateSite::Attribute(NameBuf::new("codes").unwrap())),
            failure: Box::new(ResolutionFailure::Decode {
                template: TemplateSource::new("{{ codes | split(pat=' ') }}"),
                source: serde_json::from_str::<serde_json::Value>("[").unwrap_err(),
            }),
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

        let entries = entries(sink);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.entity, urn("urn:ngsi-ld:Country:A"));
        assert_eq!(entries[0].1.occurrences.get(), 2);
    }

    #[test]
    fn two_attributes_are_two_records_in_the_order_they_failed() {
        let sink = UnresolvedTemplates::new();
        sink.record(&name("dafifCode"), &urn("urn:ngsi-ld:Country:A"), failure());
        sink.record(&name("isoCode"), &urn("urn:ngsi-ld:Country:A"), failure());

        let entries = entries(sink);
        assert_eq!(entries[0].0, name("dafifCode"));
        assert_eq!(entries[1].0, name("isoCode"));
    }
}
