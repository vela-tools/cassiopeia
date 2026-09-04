use crate::urn::error::{Result, UrnError};
use urn_rs::{Urn, UrnBuilder as UrnRsBuilder};

/// The namespace identifier every Cassiopeia entity URN carries: `ngsi-ld`.
const NID: &str = "ngsi-ld";

/// Assembles NGSI-LD entity URNs from an entity type and a cleaned identifier.
pub(crate) struct UrnBuilder;

impl UrnBuilder {
    /// Builds `urn:ngsi-ld:<entity_type>:<id>`.
    pub(crate) fn build(entity_type: &str, id: &str) -> Result<Urn> {
        let mut nss = String::with_capacity(entity_type.len() + id.len() + 1);
        nss.push_str(entity_type);
        nss.push(':');
        nss.push_str(id);

        UrnRsBuilder::new(NID, &nss).build().map_err(|source| UrnError::BuildUrn {
            nss: nss.into_boxed_str(),
            source,
        })
    }
}
