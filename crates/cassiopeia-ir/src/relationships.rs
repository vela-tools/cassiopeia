use crate::relationship_path::RelationshipPath;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use foldhash::fast::RandomState;
use indexmap::IndexMap;
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

/// The per-instance object lists of every `ListRelationship` carrying several `datasetId`-tagged
/// instances (ETSI GS CIM 009 v1.9.1 clause 4.5.5), keyed by attribute name.
///
/// One inner `Vec<Urn>` per surviving instance, in declaration order. Hashed like [`Relationships`].
pub type InstanceRelationships = IndexMap<NameBuf, Vec<Vec<Urn>>, RandomState>;
