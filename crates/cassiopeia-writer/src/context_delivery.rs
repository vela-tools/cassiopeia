/// How an entity's `@context` is delivered to the destination.
///
/// NGSI-LD lets a producer either embed the `@context` in each entity body (`application/ld+json`)
/// or reference it out of band via a `Link` header on an `application/json` payload (ETSI GS CIM 009
/// v1.9.1 clause 6.3.5). This selects between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContextDelivery {
    /// Embed the `@context` in the entity body.
    #[default]
    Body,
    /// Reference the `@context` via a `Link` header, leaving it out of the body.
    LinkHeader,
}

#[cfg(test)]
mod tests {
    use crate::context_delivery::ContextDelivery;

    #[test]
    fn the_default_embeds_the_context_in_the_body() {
        assert_eq!(ContextDelivery::default(), ContextDelivery::Body);
    }
}
