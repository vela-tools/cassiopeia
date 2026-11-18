use crate::{attribute::instance_selection::InstanceSelection, dropped_attributes::DroppedAttributes};
use cassiopeia_ir::{entity_relationships::EntityRelationships, metadata::EntityMetadata};
use cassiopeia_mapping::{
    attribute::declaration_trail::{DeclarationTrail, Nesting},
    template::{resolver::TemplateResolver, template_input::TemplateInput},
};
use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
use serde_json::Value as JsonValue;

/// The guard depth for nested attribute resolution. A mapping nested past this is treated as
/// self-referential rather than legitimately deep.
const RECURSION_LIMIT: usize = 50;

/// The state threaded through one entity's attribute resolution.
///
/// A context borrows the source record with its run variables, the shared template resolver, and
/// the entity's already-minted relationship objects (top-level, nested, and per instance). It
/// optionally borrows a metadata sink; child contexts, spawned for nested declarations, never collect
/// metadata of their own.
///
/// The batch's loss sinks are borrowed shared rather than mutably, and every child context carries
/// the same ones: a nested declaration can be refused as readily as a top-level one, and a batch
/// resolves across a Rayon pool, neither of which a `&mut` sink could cross.
pub(crate) struct ResolutionContext<'a> {
    /// The source record the entity was built from, with the run variables it is read with.
    pub(crate) input: TemplateInput<'a>,
    /// The compiled-template resolver shared across the extraction stage.
    pub(crate) resolver: &'a TemplateResolver,
    /// Relationship objects already minted for this entity. Its nested objects are `None` when the
    /// entity has none, so the nested-relationship lookup is skipped entirely (ETSI GS CIM 009 v1.9.1
    /// clause 4.5.2.2 with 4.5.3).
    pub(crate) relationships: &'a EntityRelationships,
    /// Where a refused conversion, and a null left out of a value, is recorded, shared by every
    /// context of the batch.
    pub(crate) dropped: &'a DroppedAttributes,
    /// How deep into nested attribute declarations this context sits.
    pub(crate) depth: usize,
    /// The declarations holding the ones this context resolves, or `None` at the top level, so a
    /// loss met beneath the top level is named by its whole path.
    pub(crate) owners: Option<&'a DeclarationTrail<'a>>,
    /// Which instances of a top-level attribute declaring them are rendered.
    pub(crate) instances: InstanceSelection<'a>,
    /// Where attribute-level properties are recorded, present only on the root context.
    pub(crate) metadata: Option<&'a mut EntityMetadata>,
}

impl<'a> ResolutionContext<'a> {
    /// Builds the root context for one entity's extraction.
    pub(crate) const fn new(
        input: TemplateInput<'a>,
        resolver: &'a TemplateResolver,
        relationships: &'a EntityRelationships,
        dropped: &'a DroppedAttributes,
        metadata: Option<&'a mut EntityMetadata>,
    ) -> ResolutionContext<'a> {
        ResolutionContext {
            input,
            resolver,
            relationships,
            dropped,
            depth: 0,
            owners: None,
            instances: InstanceSelection::All,
            metadata,
        }
    }

    /// The same context, rendering only the instances `instances` selects of an attribute that
    /// declares them.
    pub(crate) const fn selecting(self, instances: InstanceSelection<'a>) -> ResolutionContext<'a> {
        ResolutionContext { instances, ..self }
    }

    /// Derives a deeper context over `record`, read with the same run variables, one level down and
    /// collecting no metadata.
    pub(crate) const fn child(&self, record: &'a JsonValue) -> ResolutionContext<'a> {
        ResolutionContext {
            input: self.input.with_record(record),
            resolver: self.resolver,
            relationships: self.relationships,
            dropped: self.dropped,
            depth: self.depth + 1,
            owners: self.owners,
            instances: self.instances,
            metadata: None,
        }
    }

    /// Derives a context at the same depth for resolving the declarations `owner` holds,
    /// collecting no metadata.
    pub(crate) const fn beneath<'b>(&'b self, owner: &'b DeclarationTrail<'b>) -> ResolutionContext<'b> {
        ResolutionContext {
            input: self.input,
            resolver: self.resolver,
            relationships: self.relationships,
            dropped: self.dropped,
            depth: self.depth,
            owners: Some(owner),
            instances: self.instances,
            metadata: None,
        }
    }

    /// The path of the declaration `name`, resolved in this context.
    pub(crate) fn path_of(&self, name: &NameBuf) -> AttributePath {
        DeclarationTrail::path_of(self.owners, name)
    }

    /// The top-level attribute a loss of the declaration `name` is filed under when `name` is an
    /// `observedAt` qualifier: a sub-attribute named `observedAt`, declared on that attribute, on
    /// one of its instances, or on any sub-attribute nested beneath it.
    ///
    /// An `observedAt` qualifies the Property or Relationship it is a sub-attribute of (ETSI GS CIM
    /// 009 v1.9.1 clause 4.5.2.2), so one declared as a member of a structured value, or as a
    /// top-level attribute, qualifies nothing and has no owner.
    pub(crate) fn observed_at_owner(&self, name: &NameBuf) -> Option<&'a NameBuf> {
        let owner = self.owners?;
        match owner.nesting() {
            Nesting::SubAttribute if name.as_str() == "observedAt" => Some(owner.root()),
            Nesting::SubAttribute | Nesting::Member => None,
        }
    }

    /// Whether resolution has recursed past the guard depth.
    pub(crate) const fn recursion_limit_exceeded(&self) -> bool {
        self.depth > RECURSION_LIMIT
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute::resolution_context::ResolutionContext, dropped_attributes::DroppedAttributes};
    use cassiopeia_ir::entity_relationships::EntityRelationships;
    use cassiopeia_mapping::{
        attribute::declaration_trail::{DeclarationTrail, Nesting},
        template::{run_variables::RunVariables, runner::TemplateRunner, template_input::TemplateInput},
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).unwrap()
    }

    /// The top-level attribute a loss of the declaration `leaf` is filed under as an `observedAt`,
    /// when `leaf` is resolved beneath the declarations `owners` frames.
    fn observed_at_owner(owners: Option<&DeclarationTrail<'_>>, leaf: &str) -> Option<String> {
        let record = json!({});
        let variables = RunVariables::none();
        let resolver = TemplateRunner::new().resolver();
        let relationships = EntityRelationships::default();
        let dropped = DroppedAttributes::new();
        let context = ResolutionContext {
            owners,
            ..ResolutionContext::new(TemplateInput::new(&record, &variables), &resolver, &relationships, &dropped, None)
        };

        context.observed_at_owner(&name(leaf)).map(ToString::to_string)
    }

    #[test]
    fn a_top_level_attribute_named_observed_at_has_no_owner() {
        assert_eq!(observed_at_owner(None, "observedAt"), None);
    }

    #[test]
    fn an_observed_at_sub_attribute_is_owned_by_the_attribute_it_qualifies() {
        let temperature = name("temperature");
        let frame = DeclarationTrail::below(&temperature, Nesting::SubAttribute, None);

        assert_eq!(observed_at_owner(Some(&frame), "observedAt"), Some("temperature".to_owned()));
    }

    #[test]
    fn another_sub_attribute_has_no_owner() {
        let temperature = name("temperature");
        let frame = DeclarationTrail::below(&temperature, Nesting::SubAttribute, None);

        assert_eq!(observed_at_owner(Some(&frame), "unitCode"), None);
    }

    #[test]
    fn a_member_named_observed_at_has_no_owner() {
        let reading = name("reading");
        let frame = DeclarationTrail::below(&reading, Nesting::Member, None);

        assert_eq!(observed_at_owner(Some(&frame), "observedAt"), None);
    }

    #[test]
    fn an_observed_at_two_levels_down_is_owned_by_the_top_level_attribute() {
        let (temperature, accuracy) = (name("temperature"), name("accuracy"));
        let outer = DeclarationTrail::below(&temperature, Nesting::SubAttribute, None);
        let inner = DeclarationTrail::below(&accuracy, Nesting::SubAttribute, Some(&outer));

        assert_eq!(observed_at_owner(Some(&inner), "observedAt"), Some("temperature".to_owned()));
    }
}
