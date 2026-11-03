use cassiopeia_common::representation::NgsiLdRepresentation;

/// How a broker writer serializes each entity onto the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BrokerSerialization {
    /// The NGSI-LD representation entities are serialized in.
    pub representation: NgsiLdRepresentation,
}
