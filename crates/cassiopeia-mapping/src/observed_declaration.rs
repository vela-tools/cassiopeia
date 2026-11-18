use crate::conversion::Conversion;
use cassiopeia_ngsi_ld::entity::name::NameBuf;

/// One `observedAt` sub-attribute a mapping declares, named by where it sits.
///
/// A record whose `observedAt` text reads as no instant costs each declaration reading it a loss,
/// and which loss depends on how the declaration converts its value and on the attribute it
/// qualifies, so both are kept beside the name the sub-attribute is declared under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedDeclaration {
    /// The top-level attribute the sub-attribute qualifies, directly or through the attributes
    /// nested beneath it.
    pub attribute: NameBuf,
    /// The name the sub-attribute is declared under.
    pub observed_at: NameBuf,
    /// How the sub-attribute converts the value it reads.
    pub conversion: Conversion,
}
