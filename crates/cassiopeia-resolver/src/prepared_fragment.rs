//! One fragment's store writes worked out ahead of them: the part of resolving a fragment that is the
//! same whichever write path then stores it.

use crate::{
    entity_store::{store::FragmentWrite, write_batch::WriteBatch},
    error::Result,
    fragment_resolver::FragmentResolver,
    mapping_id::MappingId,
};
use cassiopeia_ir::{
    fragment::{Fragment, FragmentParts},
    mapped::Mapped,
    source_position::SourcePosition,
};
use cassiopeia_mapping::{
    mapping::Mapping,
    observed_at::ObservedAt,
    template::{error::TemplateError, template_input::TemplateInput},
};
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{result, sync::Arc};
use urn_rs::Urn;

/// One fragment taken apart for its store writes, with its record's `observedAt` read and its
/// relationship edges built.
///
/// Every field is owned: the resolver is the fragment's last holder, so preparation takes the record
/// and the scope apart rather than borrowing them, and the store write moves them into place.
pub(crate) struct PreparedFragment {
    /// The entity the fragment contributes to.
    pub(crate) base_id: Urn,
    /// The source record the fragment was expanded from.
    pub(crate) source_data: Value,
    /// Where that record stands in the run's sources.
    pub(crate) position: SourcePosition,
    /// The entity's scope, when one was resolved.
    pub(crate) scope: Option<NgsiLdScope>,
    /// The entity's `expiresAt`, when one was resolved.
    pub(crate) expires_at: Option<DateTime<Utc>>,
    /// The mapping that produced the fragment, whose read set the entity write borrows.
    pub(crate) mapping: Arc<Mapping>,
    /// The interned id of the mapping read with the fragment's run variables.
    pub(crate) mapping_id: MappingId,
    /// What the record-level `observedAt` template yielded. It is read as a ranking timestamp only
    /// when the fragment is written, since a failure is kept against the mapping in the order the
    /// writes happen.
    pub(crate) observed_at: result::Result<Option<ObservedAt>, TemplateError>,
    /// Whether the mapping is temporal.
    pub(crate) temporal: bool,
    /// The parent-child edges the fragment records.
    pub(crate) edges: Vec<(Urn, String, Urn)>,
}

impl FragmentResolver {
    /// Takes `fragment`, produced by the mapping interned as `mapping_id`, apart for its store
    /// writes.
    pub(crate) fn prepare(&self, fragment: Mapped<Fragment>, mapping_id: MappingId) -> PreparedFragment {
        let (fragment, mapping) = fragment.into_parts();
        let observed_at = mapping.extract_observed_at(&self.resolver, TemplateInput::new(fragment.source_data(), fragment.variables()));
        let temporal = mapping.is_temporal();
        // The variables were interned into the mapping id, which is what the store keeps.
        let FragmentParts {
            source_data,
            target_urn: base_id,
            position,
            scope,
            expires_at,
            parent_context,
            variables: _,
        } = fragment.into_parts();
        let edges = self.build_edges(&base_id, parent_context.as_ref(), temporal);

        PreparedFragment {
            base_id,
            source_data,
            position,
            scope,
            expires_at,
            mapping,
            mapping_id,
            observed_at,
            temporal,
            edges,
        }
    }

    /// Writes one prepared fragment's relationship edges, then the fragment itself, each straight to
    /// its store, as part of `batch`.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError`](crate::error::ResolverError) when either store write fails.
    pub(crate) fn write_prepared(&self, prepared: PreparedFragment, batch: WriteBatch) -> Result<()> {
        let PreparedFragment {
            base_id,
            source_data,
            position,
            scope,
            expires_at,
            mapping,
            mapping_id,
            observed_at,
            temporal,
            edges,
        } = prepared;
        for (parent, property, child) in &edges {
            self.relationship_store.add_child(parent, property, child)?;
        }
        let timestamp = self.record_timestamp(mapping_id, observed_at);
        self.entity_store.store_fragment(FragmentWrite {
            base_id,
            source_data,
            position,
            batch,
            mapping_id,
            temporal,
            timestamp,
            scope,
            expires_at,
            reads: mapping.source_reads(),
        })?;
        Ok(())
    }
}
