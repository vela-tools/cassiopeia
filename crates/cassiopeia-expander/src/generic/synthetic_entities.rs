use crate::{error::ExpanderError, expansion::Expansion, generic::GenericExpander};
use cassiopeia_ir::{
    fragment::Fragment,
    mapped::Mapped,
    parent_context::{ParentContext, ParentContextType},
    relationship_path::RelationshipPath,
};
use cassiopeia_mapping::{attribute::Attribute, mapping::Mapping};
use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};
use serde_json::{Map, Value};
use std::sync::Arc;

/// The related entities a record describes beside its main entity, each declared by the
/// `syntheticEntity` of the attribute that links the main entity to it.
impl GenericExpander {
    /// Expands the synthetic entities every attribute of `mapping` declares into `entities`, each in
    /// its own place, and returns the links the main entity holds to the per-token targets of its
    /// list relationships, since they belong to the main entity's own fragment.
    pub(super) fn expand_synthetic_entities(
        &self,
        mapping: &Mapping,
        expansion: &Expansion<'_>,
        entities: &mut Vec<Result<Mapped<Fragment>, ExpanderError>>,
    ) -> Vec<ParentContext> {
        let mut links = Vec::new();
        for (attribute_name, attribute) in mapping.attributes() {
            let Some(synthetic) = attribute.synthetic_entity() else {
                continue;
            };
            if attribute.target().is_some() && matches!(attribute.kind(), NgsiLdAttributeKind::ListRelationship) {
                links.extend(self.expand_synthetic_targets(attribute_name, attribute, synthetic, expansion, entities));
            } else {
                entities.extend(
                    self.expand_synthetic_entity(attribute_name, attribute, synthetic, expansion, entities.len())
                        .transpose(),
                );
            }
        }
        links
    }

    /// Materialises one target per identifier of a list relationship that also declares a synthetic
    /// entity, each in its own place among `entities`, and returns the links to the targets that are
    /// written.
    ///
    /// The field is split into its tokens, and each token both links the main entity to a target and
    /// is emitted as that target's own entity. Minting the id once and reusing it for the link keeps
    /// the relationship and the emitted entity in step, so a token whose synthetic entity is not
    /// written, because its identity names no target or its scope or `expiresAt` fails, contributes
    /// no link either: the rest of the list, and the main entity, are still produced.
    fn expand_synthetic_targets(
        &self,
        attribute_name: &NameBuf,
        attribute: &Attribute,
        synthetic: &Arc<Mapping>,
        expansion: &Expansion<'_>,
        entities: &mut Vec<Result<Mapped<Fragment>, ExpanderError>>,
    ) -> Vec<ParentContext> {
        let path = || RelationshipPath::flat(attribute_name.clone());
        let tokens = match expansion.keep(self.urn_generator.source_identifiers(attribute, expansion.input), path) {
            Ok(tokens) => tokens.unwrap_or_default(),
            Err(error) => {
                entities.push(Err(error));
                return Vec::new();
            }
        };

        let mut links = Vec::new();
        for token in tokens {
            let target = self.expand_synthetic_target(attribute_name, synthetic, token, expansion, entities.len());
            if let Ok(Some(fragment)) = &target {
                links.push(ParentContext::new(
                    ParentContextType::Child(fragment.inner().target_urn().clone()),
                    Self::top_level_key(attribute_name),
                ));
            }
            entities.extend(target.transpose());
        }
        links
    }

    /// Expands the synthetic entity one `token` of a list relationship materialises, taking the place
    /// of the record's entity at `ordinal`, or nothing when the token names no target.
    ///
    /// The token is bound as a single-column positional record, so the synthetic mapping reads it
    /// with the same `this[0]` accessor a headerless source uses.
    fn expand_synthetic_target(
        &self,
        attribute_name: &NameBuf,
        synthetic: &Arc<Mapping>,
        token: String,
        expansion: &Expansion<'_>,
        ordinal: usize,
    ) -> Result<Option<Mapped<Fragment>>, ExpanderError> {
        let token_data = Value::Object(Map::from_iter([("0".to_owned(), Value::String(token))]));
        let token_input = expansion.input.with_record(&token_data);
        let path = || RelationshipPath::flat(attribute_name.clone());
        let Some(synthetic_urn) = expansion.keep(self.urn_generator.generate_id(synthetic, token_input), path)? else {
            return Ok(None);
        };
        let head = self.head(synthetic, synthetic_urn, token_input, expansion.losses)?;

        let fragment = self.fragment(token_data, head, expansion.position.nth_fragment(ordinal), None);
        Ok(Some(Mapped::new(fragment, Arc::clone(synthetic))))
    }

    /// Expands the one synthetic entity a non-list attribute materialises, linked back to the main
    /// entity through a parent context when the main entity is written, or nothing when this record
    /// names no target. The fragment takes the place of the record's entity at `ordinal`.
    ///
    /// The link the main entity carries is minted from the synthetic identity, so it always points at
    /// the entity emitted here. A record whose synthetic identity, or whose relationship source,
    /// resolves to no identifier or fails to render has no target to emit or link: the attribute
    /// contributes nothing and the main entity is still produced, exactly as a plain Relationship
    /// with an absent foreign key and a `ListRelationship` with no tokens behave.
    fn expand_synthetic_entity(
        &self,
        attribute_name: &NameBuf,
        attribute: &Attribute,
        synthetic: &Arc<Mapping>,
        expansion: &Expansion<'_>,
        ordinal: usize,
    ) -> Result<Option<Mapped<Fragment>>, ExpanderError> {
        let path = || RelationshipPath::flat(attribute_name.clone());
        // The minted object is discarded: this only asks whether the relationship source names a
        // target, under the very definition the plain Relationship path uses.
        if attribute.target().is_some()
            && attribute.source().is_some()
            && expansion
                .keep(self.urn_generator.generate_child_id(attribute, expansion.input), path)?
                .is_none()
        {
            return Ok(None);
        }

        let Some(synthetic_urn) = expansion.keep(self.urn_generator.generate_id(synthetic, expansion.input), path)? else {
            return Ok(None);
        };
        let head = self.head(synthetic, synthetic_urn, expansion.input, expansion.losses)?;

        // The parent context owns its URN and path, while the main entity keeps its own for its fragment.
        let parent = expansion
            .holder
            .map(|main| vec![ParentContext::new(ParentContextType::Parent(main.clone()), Self::top_level_key(attribute_name))]);
        // Each synthetic fragment carries its own copy of the source data; the main fragment consumes
        // the record once every attribute has been expanded.
        let fragment = self.fragment(expansion.input.record().clone(), head, expansion.position.nth_fragment(ordinal), parent);
        Ok(Some(Mapped::new(fragment, Arc::clone(synthetic))))
    }
}
