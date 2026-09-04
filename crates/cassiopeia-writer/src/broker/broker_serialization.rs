use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};

/// How a broker writer serializes each entity onto the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BrokerSerialization {
    /// The NGSI-LD representation entities are serialized in.
    pub representation: NgsiLdRepresentation,
    /// Whether null-valued attributes are skipped during serialization.
    pub skip_null: NgsiLdSkipNull,
}
