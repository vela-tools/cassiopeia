use crate::{instance_index::InstanceIndex, relationship_path::RelationshipPath};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use foldhash::fast::RandomState;
use indexmap::IndexMap;
use std::collections::BTreeMap;
use urn_rs::Urn;

/// An entity's top-level relationship targets, keyed by attribute name in declaration order.
///
/// The keys are attribute names a mapping declared (trusted configuration rather than
/// attacker-controlled input), and every relationship of every record probes this map, so it hashes
/// with `foldhash` rather than the standard library's `SipHash`.
pub type Relationships = IndexMap<NameBuf, Vec<Urn>, RandomState>;

/// The targets of relationships declared as sub-attributes, keyed by their path from the entity
/// (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with 4.5.3).
///
/// Every key is a multi-segment path; a single-segment (top-level) relationship lives in
/// [`Relationships`] instead. Hashed the same way, and for the same reason: the path segments are
/// attribute names the mapping declared.
pub type NestedRelationships = IndexMap<RelationshipPath, Vec<Urn>, RandomState>;

/// The objects each instance of one multi-attribute relationship minted, keyed by the instance's
/// declaration index (ETSI GS CIM 009 v1.9.1 clause 4.5.5).
///
/// Ordered by index, so the instances come out in declaration order. An instance that minted no
/// object has no entry, so it is omitted without moving any other instance off its own index.
pub type InstanceObjects = BTreeMap<InstanceIndex, Vec<Urn>>;

/// The per-instance objects of every multi-attribute `Relationship` or `ListRelationship` (ETSI GS
/// CIM 009 v1.9.1 clause 4.5.5, EXAMPLE 19), keyed by attribute name. Hashed like [`Relationships`].
pub type InstanceRelationships = IndexMap<NameBuf, InstanceObjects, RandomState>;
