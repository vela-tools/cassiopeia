use serde::{Deserialize, Serialize};
use strum::Display;

/// How the `@context` reaches a Context Broker.
///
/// NGSI-LD allows a request to either embed `@context` in the JSON-LD body or reference it from a
/// `Link` header (ETSI GS CIM 009 v1.9.1 clause 6.3.5); brokers differ in which they accept, so the
/// manifest states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Display, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ContextDelivery {
    /// Embed `@context` in the entity body.
    #[default]
    Body,

    /// Reference `@context` from a `Link` header and keep it out of the body.
    LinkHeader,
}

impl ContextDelivery {
    /// Chooses the delivery a `--link-header` presence flag selects.
    ///
    /// A set flag references `@context` from a `Link` header; an unset flag embeds it in the body.
    #[must_use]
    pub const fn from_link_header(link_header: bool) -> ContextDelivery {
        if link_header { ContextDelivery::LinkHeader } else { ContextDelivery::Body }
    }
}

#[cfg(test)]
mod tests {
    use crate::output::context_delivery::ContextDelivery;

    #[test]
    fn the_wire_form_is_kebab_case() {
        assert_eq!(serde_json::to_string(&ContextDelivery::LinkHeader).unwrap(), r#""link-header""#);
        assert_eq!(serde_json::from_str::<ContextDelivery>(r#""body""#).unwrap(), ContextDelivery::Body);
    }

    #[test]
    fn an_unstated_delivery_embeds_the_context_in_the_body() {
        assert_eq!(ContextDelivery::default(), ContextDelivery::Body);
    }

    #[test]
    fn a_set_link_header_flag_references_the_context_from_a_header() {
        assert_eq!(ContextDelivery::from_link_header(true), ContextDelivery::LinkHeader);
    }

    #[test]
    fn an_unset_link_header_flag_embeds_the_context_in_the_body() {
        assert_eq!(ContextDelivery::from_link_header(false), ContextDelivery::Body);
    }
}
