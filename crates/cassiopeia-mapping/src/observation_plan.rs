use crate::{
    attribute::{Attribute, Attributes},
    error::{MappingError, Result},
    observation_slot::{AttributeSlots, SlotIndex},
    observed_declaration::ObservedDeclaration,
    observed_template::{ObservedTemplate, ObservedTemplateIndex},
    template::{TemplateSource, error::TemplateError, resolver::TemplateResolver, runner::TemplateRunner, template_input::TemplateInput},
    template_location::TemplateLocation,
    template_site::TemplateSite,
};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use serde_json::Value as JsonValue;
use smallvec::SmallVec;
use std::{path::Path, result, sync::Arc};

/// What each distinct `observedAt` template of a mapping rendered for one record, in the order of
/// [`ObservationPlan::templates`]. A mapping declares one template in the common case, which the
/// inline capacity keeps off the heap.
pub type RenderedObservations = SmallVec<[result::Result<JsonValue, TemplateError>; 1]>;

/// How a mapping's records are ranked against each other when several resolve to one entity: the
/// distinct `observedAt` templates it declares, and which of them ranks each top-level attribute.
///
/// ETSI GS CIM 009 v1.9.1 clause 4.5.5.3 resolves conflicting instances of an attribute per
/// attribute, keeping the one with the most recent `observedAt`. Each distinct template a top-level
/// attribute or one of its instances carries as its own is therefore a slot of its own, and every
/// attribute it ranks comes from the record where it names the latest instant; an instance's own
/// `observedAt` overrides the attribute's, as its properties overlay the attribute's. Everything else,
/// an attribute without an `observedAt` of its own included, comes from the record observed last
/// over every template the mapping declares, nested ones included.
///
/// A mapping with one distinct template has no slots: every slot would rank by the same instant,
/// with the same tie-break, as the record observed last, so that one record is all a store keeps.
#[derive(Debug, Clone, Default)]
pub struct ObservationPlan {
    /// Every distinct template, in the order the mapping first declares it.
    templates: Vec<ObservedTemplate>,
    /// The template each slot ranks by, slot by slot. Empty unless the mapping declares more than one
    /// distinct template.
    slots: Vec<ObservedTemplateIndex>,
    /// The slots ranking each top-level attribute, by its position among the mapping's attributes.
    /// Empty whenever `slots` is.
    by_attribute: Vec<AttributeSlots>,
}

/// The templates a top-level attribute and its instances carry as their own `observedAt`, before
/// they are numbered as slots.
struct AttributeTemplates {
    /// The template of an attribute that declares no instances.
    declaration: Option<ObservedTemplateIndex>,
    /// The template of each declared instance: its own, or else the attribute's.
    instances: Vec<Option<ObservedTemplateIndex>>,
}

/// The distinct templates met while walking a mapping, with the declarations reading each.
#[derive(Default)]
struct TemplateTable {
    entries: Vec<(TemplateSource, Vec<ObservedDeclaration>)>,
}

impl TemplateTable {
    /// Notes that the `observedAt` sub-attribute `observed_at`, named `key` and qualifying the
    /// top-level attribute `attribute`, reads its template, and returns the template's index, or
    /// `None` when the sub-attribute declares no template.
    fn note(&mut self, attribute: &NameBuf, (key, observed_at): (&NameBuf, &Attribute)) -> Option<ObservedTemplateIndex> {
        let source = observed_at.declared_source()?;
        let declaration = ObservedDeclaration {
            attribute: attribute.clone(),
            observed_at: key.clone(),
            conversion: observed_at.conversion(),
        };
        let index = if let Some(index) = self.entries.iter().position(|(known, _)| *known == source) {
            index
        } else {
            self.entries.push((source, Vec::new()));
            self.entries.len() - 1
        };
        if let Some((_, declarations)) = self.entries.get_mut(index) {
            declarations.push(declaration);
        }
        Some(ObservedTemplateIndex::new(index))
    }

    /// Notes every `observedAt` declared on `declaration` or anything nested beneath it, all of
    /// which qualify the top-level attribute `attribute`.
    fn note_beneath(&mut self, attribute: &NameBuf, declaration: &Attribute) {
        for nested in declaration.iter_recursive() {
            if let Some(observed_at) = nested.observed_at() {
                self.note(attribute, observed_at);
            }
        }
    }

    /// Notes every `observedAt` the top-level attribute `name` declares, and returns the templates it
    /// and its instances carry as their own.
    fn note_attribute(&mut self, name: &NameBuf, attribute: &Attribute) -> AttributeTemplates {
        let shared = attribute.observed_at().and_then(|observed_at| self.note(name, observed_at));
        // The attribute's own `observedAt` was noted above; the walk notes what is nested beneath it.
        for nested in attribute.iter_recursive().skip(1) {
            if let Some(observed_at) = nested.observed_at() {
                self.note(name, observed_at);
            }
        }
        let Some(instances) = attribute.instances() else {
            return AttributeTemplates {
                declaration: shared,
                instances: Vec::new(),
            };
        };
        let instances = instances
            .iter()
            .map(|instance| {
                let properties = instance.properties().as_ref();
                let own = properties
                    .and_then(|properties| properties.get_key_value("observedAt"))
                    .and_then(|observed_at| self.note(name, observed_at));
                for property in properties.into_iter().flat_map(Attributes::values) {
                    self.note_beneath(name, property);
                }
                own.or(shared)
            })
            .collect();
        AttributeTemplates { declaration: None, instances }
    }
}

impl ObservationPlan {
    /// The plan of a mapping declaring `attributes`, compiling each distinct `observedAt` template
    /// once against `runner`. `origin` names the mapping document in a compile error.
    ///
    /// # Errors
    /// Returns [`MappingError::UncompilableTemplate`] when an `observedAt` template cannot be
    /// compiled.
    pub fn new(attributes: &Attributes, origin: &Path, runner: &mut TemplateRunner) -> Result<ObservationPlan> {
        let mut table = TemplateTable::default();
        let per_attribute: Vec<AttributeTemplates> = attributes.iter().map(|(name, attribute)| table.note_attribute(name, attribute)).collect();

        let location = TemplateLocation::new(Arc::from(origin), TemplateSite::ObservedAt);
        let templates = table
            .entries
            .into_iter()
            .map(|(source, declarations)| {
                let compiled = runner.compile(&source, &location).map_err(|error| MappingError::UncompilableTemplate {
                    location: location.clone(),
                    source: error,
                })?;
                Ok(ObservedTemplate::new(source, compiled, declarations))
            })
            .collect::<Result<Vec<ObservedTemplate>>>()?;

        if templates.len() < 2 {
            return Ok(ObservationPlan {
                templates,
                slots: Vec::new(),
                by_attribute: Vec::new(),
            });
        }
        let mut slots = Vec::new();
        let mut slot_of = |template: Option<ObservedTemplateIndex>| {
            let template = template?;
            let index = slots.iter().position(|known| *known == template).unwrap_or_else(|| {
                slots.push(template);
                slots.len() - 1
            });
            Some(SlotIndex::new(index))
        };
        let by_attribute = per_attribute
            .into_iter()
            .map(|templates| AttributeSlots::new(slot_of(templates.declaration), templates.instances.into_iter().map(&mut slot_of).collect()))
            .collect();
        Ok(ObservationPlan {
            templates,
            slots,
            by_attribute,
        })
    }

    /// Whether the mapping declares an `observedAt` anywhere, so its records are ranked rather than
    /// merged.
    #[must_use]
    pub const fn is_temporal(&self) -> bool {
        !self.templates.is_empty()
    }

    /// Every distinct template, in the order the mapping first declares it.
    #[must_use]
    pub fn templates(&self) -> &[ObservedTemplate] {
        &self.templates
    }

    /// The template at `index`.
    #[must_use]
    pub fn template(&self, index: ObservedTemplateIndex) -> Option<&ObservedTemplate> {
        self.templates.get(index.get())
    }

    /// The template each slot ranks by, slot by slot; empty when the mapping declares at most one
    /// distinct template.
    #[must_use]
    pub fn slot_templates(&self) -> &[ObservedTemplateIndex] {
        &self.slots
    }

    /// The slots ranking the top-level attribute at `attribute` among the mapping's attributes, or
    /// `None` when the mapping has no slots.
    #[must_use]
    pub fn slots_of(&self, attribute: usize) -> Option<&AttributeSlots> {
        self.by_attribute.get(attribute)
    }

    /// Renders every distinct template once for one record, read with its run variables.
    #[must_use]
    pub fn observe(&self, resolver: &TemplateResolver, input: TemplateInput<'_>) -> RenderedObservations {
        self.templates.iter().map(|template| resolver.resolve(template.compiled(), input)).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::Attributes,
        observation_plan::ObservationPlan,
        observation_slot::{AttributeSlots, SlotIndex},
        observed_template::ObservedTemplateIndex,
        template::{TemplateSource, run_variables::RunVariables, runner::TemplateRunner, template_input::TemplateInput},
        template_site::TemplateSite,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::path::Path;

    fn plan(attributes: &str) -> (ObservationPlan, TemplateRunner) {
        let attributes: Attributes = serde_json5::from_str(attributes).unwrap();
        let mut runner = TemplateRunner::new();
        (ObservationPlan::new(&attributes, Path::new("sensor.json5"), &mut runner).unwrap(), runner)
    }

    fn sources(plan: &ObservationPlan) -> Vec<String> {
        plan.templates().iter().map(|template| template.source().to_string()).collect()
    }

    #[test]
    fn a_template_shared_by_two_attributes_is_one_template_and_no_slot() {
        let (plan, _) = plan(
            r#"{
                reading: { source: "{{ r }}", properties: { observedAt: { source: "{{ time }}" } } },
                status: { source: "{{ s }}", properties: { observedAt: { source: "{{ time }}" } } },
            }"#,
        );

        assert!(plan.is_temporal());
        assert_eq!(sources(&plan), ["{{ time }}"]);
        assert_eq!(plan.templates()[0].declarations().len(), 2);
        assert_eq!(plan.slot_templates(), &[]);
        assert_eq!(plan.slots_of(0), None);
    }

    #[test]
    fn two_distinct_templates_rank_their_attributes_in_a_slot_each_and_leave_the_rest_unranked() {
        let (plan, _) = plan(
            r#"{
                reading: { source: "{{ r }}", properties: { observedAt: { source: "{{ read_time }}" } } },
                status: { source: "{{ s }}", properties: { observedAt: { source: "{{ status_time }}" } } },
                name: { source: "{{ name }}" },
            }"#,
        );

        assert_eq!(sources(&plan), ["{{ read_time }}", "{{ status_time }}"]);
        assert_eq!(plan.slot_templates(), &[ObservedTemplateIndex::new(0), ObservedTemplateIndex::new(1)]);
        assert_eq!(plan.slots_of(0), Some(&AttributeSlots::new(Some(SlotIndex::new(0)), Vec::new())));
        assert_eq!(plan.slots_of(1), Some(&AttributeSlots::new(Some(SlotIndex::new(1)), Vec::new())));
        assert_eq!(plan.slots_of(2), Some(&AttributeSlots::new(None, Vec::new())));
    }

    #[test]
    fn each_instance_is_ranked_by_its_own_template_or_else_the_attribute_s() {
        let (plan, _) = plan(
            r#"{
                forecast: {
                    properties: { observedAt: { source: "{{ tc }}" } },
                    instances: [
                        { source: "{{ a }}", properties: { datasetId: { source: "urn:ngsi-ld:Dataset:a" }, observedAt: { source: "{{ ta }}" } } },
                        { source: "{{ b }}", properties: { datasetId: { source: "urn:ngsi-ld:Dataset:b" }, observedAt: { source: "{{ tb }}" } } },
                        { source: "{{ c }}", properties: { datasetId: { source: "urn:ngsi-ld:Dataset:c" } } },
                    ],
                },
            }"#,
        );

        assert_eq!(sources(&plan), ["{{ tc }}", "{{ ta }}", "{{ tb }}"]);
        assert_eq!(
            plan.slot_templates(),
            &[ObservedTemplateIndex::new(1), ObservedTemplateIndex::new(2), ObservedTemplateIndex::new(0)]
        );
        assert_eq!(
            plan.slots_of(0),
            Some(&AttributeSlots::new(
                None,
                vec![Some(SlotIndex::new(0)), Some(SlotIndex::new(1)), Some(SlotIndex::new(2))]
            ))
        );
    }

    #[test]
    fn an_observed_at_declared_only_on_an_instance_makes_the_mapping_temporal() {
        let (plan, _) = plan(r#"{ reading: { instances: [{ source: "{{ a }}", properties: { observedAt: { source: "{{ ta }}" } } }] } }"#);

        assert!(plan.is_temporal());
        assert_eq!(sources(&plan), ["{{ ta }}"]);
        assert_eq!(plan.templates()[0].declarations()[0].attribute.as_str(), "reading");
    }

    #[test]
    fn an_observed_at_declared_only_on_a_sub_attribute_is_one_template_and_no_slot() {
        let (plan, _) =
            plan(r#"{ reading: { source: "{{ r }}", properties: { accuracy: { source: "{{ a }}", properties: { observedAt: { source: "{{ ta }}" } } } } } }"#);

        assert!(plan.is_temporal());
        assert_eq!(sources(&plan), ["{{ ta }}"]);
        assert_eq!(plan.slot_templates(), &[]);
    }

    #[test]
    fn a_mapping_without_observed_at_is_not_temporal() {
        let (plan, _) = plan(r#"{ reading: { source: "{{ r }}" } }"#);

        assert!(!plan.is_temporal());
        assert_eq!(plan.templates().len(), 0);
    }

    #[test]
    fn one_template_written_twice_is_one_template() {
        let (plan, _) = plan(
            r#"{
                reading: { source: "{{ r }}", properties: { observedAt: { source: "{{ time }}" } } },
                status: { instances: [{ source: "{{ s }}", properties: { observedAt: { source: "{{ time }}" } } }] },
            }"#,
        );

        assert_eq!(sources(&plan), ["{{ time }}"]);
        assert_eq!(plan.templates()[0].source(), &TemplateSource::new("{{ time }}"));
    }

    #[test]
    fn every_template_renders_once_per_record_and_a_failing_one_leaves_the_others_rendered() {
        let (plan, runner) = plan(
            r#"{
                reading: { source: "{{ r }}", properties: { observedAt: { source: "{{ read_time }}" } } },
                status: { source: "{{ s }}", properties: { observedAt: { source: "{{ status_time | trim }}" } } },
            }"#,
        );
        let record = json!({"read_time": "2026-03-01T11:00:00Z", "status_time": null});

        let rendered = plan.observe(&runner.resolver(), TemplateInput::new(&record, &RunVariables::none()));

        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[0].as_ref().ok(), Some(&json!("2026-03-01T11:00:00Z")));
        assert!(matches!(&rendered[1], Err(error) if error.location.site == TemplateSite::ObservedAt));
    }

    #[test]
    fn an_uncompilable_template_is_refused_naming_the_document() {
        let attributes: Attributes =
            serde_json5::from_str(r#"{ reading: { source: "{{ r }}", properties: { observedAt: { source: "{{ ts | upper " } } } }"#).unwrap();

        let error = ObservationPlan::new(&attributes, Path::new("sensor.json5"), &mut TemplateRunner::new()).unwrap_err();

        assert!(error.to_string().contains("sensor.json5"), "{error}");
    }
}
