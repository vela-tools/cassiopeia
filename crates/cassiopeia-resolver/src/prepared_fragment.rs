//! One fragment's store writes worked out ahead of them: the part of resolving a fragment that is the
//! same whichever write path then stores it.

use crate::{
    entity_store::{retention::Retention, store::FragmentWrite, write_batch::WriteBatch},
    error::Result,
    fragment_resolver::FragmentResolver,
    mapping_id::MappingId,
};
use cassiopeia_ir::{
    fragment::{Fragment, FragmentParts},
    mapped::Mapped,
    source_position::SourcePosition,
};
use cassiopeia_mapping::{mapping::Mapping, observation_plan::RenderedObservations, template::template_input::TemplateInput};
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use urn_rs::Urn;

/// One fragment taken apart for its store writes, with its record's `observedAt` templates rendered
/// and its relationship edges built.
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
    /// What each of the mapping's `observedAt` templates rendered, `None` when the record is not
    /// ranked. It is read as what the record ranks by only when the fragment is written, since a
    /// failure is kept against the mapping in the order the writes happen.
    pub(crate) observed: Option<RenderedObservations>,
    /// The parent-child edges the fragment records.
    pub(crate) edges: Vec<(Urn, String, Urn)>,
}

impl FragmentResolver {
    /// Takes `fragment`, produced by the mapping interned as `mapping_id`, apart for its store
    /// writes.
    pub(crate) fn prepare(&self, fragment: Mapped<Fragment>, mapping_id: MappingId) -> PreparedFragment {
        let (fragment, mapping) = fragment.into_parts();
        let temporal = mapping.is_temporal();
        // Only a store that keeps the current state ranks records, so a series run renders no
        // observation time at all.
        let observed = match self.retention {
            Retention::CurrentState if temporal => Some(
                mapping
                    .observation_plan()
                    .observe(&self.resolver, TemplateInput::new(fragment.source_data(), fragment.variables())),
            ),
            Retention::CurrentState | Retention::Series => None,
        };
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
            observed,
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
            observed,
            edges,
        } = prepared;
        for (parent, property, child) in &edges {
            self.relationship_store.add_child(parent, property, child)?;
        }
        let ranking = self.record_ranking(mapping_id, mapping.observation_plan(), observed);
        self.entity_store.store_fragment(FragmentWrite {
            base_id,
            source_data,
            position,
            batch,
            mapping_id,
            ranking,
            scope,
            expires_at,
            reads: mapping.source_reads(),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{dashmap_latest_store::DashMapLatestEntityStore, dashmap_series_store::DashMapSeriesEntityStore, store::EntityStore},
        fragment_resolver::FragmentResolver,
        mapping_id::MappingId,
        relationship_store::dashmap_store::DashMapRelationshipStore,
        test_order::next_position,
    };
    use cassiopeia_ir::{fragment::Fragment, mapped::Mapped};
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::{path::Path, sync::Arc};

    const TEMPORAL: &str = r#"{
        version: "v4",
        dataModel: "Sensor",
        identity: { entityName: "{{ id }}" },
        attributes: {
            reading: { source: "{{ r }}", properties: { observedAt: { source: "{{ read_time }}" } } },
            status: { source: "{{ s }}", properties: { observedAt: { source: "{{ status_time }}" } } },
        },
    }"#;

    const STATIC: &str = r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "{{ id }}" }, attributes: { r: { source: "{{ r }}" } } }"#;

    /// How many observation times preparing one record of `document` over `store` rendered, `None`
    /// when it rendered none.
    fn rendered(store: Box<dyn EntityStore>, document: &str) -> Option<usize> {
        let mut runner = TemplateRunner::new();
        let mapping = Arc::new(Mapping::from_json5(document, Path::new("sensor.json5"), &mut runner).unwrap());
        let resolver = FragmentResolver::new(store, Box::new(DashMapRelationshipStore::new()), runner.resolver());
        let fragment = Fragment::builder()
            .source_data(json!({"id": "S1", "r": 1, "s": "ok", "read_time": "2026-03-01T11:00:00Z", "status_time": "2026-03-01T12:00:00Z"}))
            .target_urn("urn:ngsi-ld:Sensor:S1".parse().unwrap())
            .position(next_position())
            .build();
        resolver
            .prepare(Mapped::new(fragment, mapping), MappingId::new(1))
            .observed
            .map(|observed| observed.len())
    }

    #[test]
    fn a_current_state_store_has_every_template_of_a_temporal_mapping_rendered_once() {
        assert_eq!(rendered(Box::new(DashMapLatestEntityStore::new()), TEMPORAL), Some(2));
    }

    #[test]
    fn a_series_store_has_no_observation_time_rendered() {
        assert_eq!(rendered(Box::new(DashMapSeriesEntityStore::new()), TEMPORAL), None);
    }

    #[test]
    fn a_static_mapping_has_no_observation_time_rendered() {
        assert_eq!(rendered(Box::new(DashMapLatestEntityStore::new()), STATIC), None);
    }
}
