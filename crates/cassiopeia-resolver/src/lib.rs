//! Resolver stage for the Cassiopeia pipeline.
//!
//! The resolver exposes two single-capability traits:
//!
//! - **Fragment sink**: incoming fragments are resolved via
//!   [`FragmentSink::resolve`](crate::fragment_sink::FragmentSink::resolve), storing source data and
//!   tracking parent-child relationships between entities.
//! - **Entity source**: stored fragments are assembled into complete
//!   [`Entity`](cassiopeia_ir::entity::Entity) values via
//!   [`EntitySource::assemble`](crate::entity_source::EntitySource::assemble), combining source
//!   data, scopes, and relationships. A caller drives assembly over the whole store through
//!   [`EntitySource::drive_assembly`](crate::entity_source::EntitySource::drive_assembly), which
//!   owns the threading and channel plumbing while the resolver contributes only the assembly work.
//!   Assembly is also where records of one mapping that resolved to the same id and disagreed are
//!   found: each is a [`RecordMerge`](crate::record_merge::RecordMerge), tallied by type into
//!   [`MergedRecords`](crate::merged_records::MergedRecords) for the caller to report.
//!
//! [`FragmentResolver`](crate::fragment_resolver::FragmentResolver) is the standard implementation
//! of both traits, backed by pluggable entity and relationship stores. Its two halves live in
//! separate modules: [`fragment_resolver`] owns the write path, [`entity_assembly`] the read-back.

pub mod coalesced_batch;
pub mod entity_assembly;
pub mod entity_source;
pub mod entity_store;
pub mod error;
pub mod field_path;
pub mod fragment_resolver;
pub mod fragment_sink;
pub mod id_assembly;
pub mod mapping_id;
pub mod mapping_registry;
pub mod merged_records;
pub mod record_merge;
pub mod relationship_store;
pub mod store_action;
pub mod store_batch_failure;
pub mod store_write_strategy;
