/// Whether the broker client assumes HTTP/2 without ALPN negotiation.
///
/// Prior knowledge (h2c) is required only for plaintext HTTP/2 brokers; over HTTPS, ALPN negotiates
/// the protocol, so this stays [`Http2PriorKnowledge::Off`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Http2PriorKnowledge {
    /// Assume HTTP/2 without negotiation (plaintext h2c brokers).
    On,
    /// Negotiate the protocol normally (the default, correct for HTTPS).
    #[default]
    Off,
}

impl Http2PriorKnowledge {
    /// Whether prior knowledge is enabled.
    #[must_use]
    pub const fn is_on(self) -> bool {
        matches!(self, Http2PriorKnowledge::On)
    }
}

#[cfg(test)]
mod tests {
    use crate::broker::http2_prior_knowledge::Http2PriorKnowledge;

    #[test]
    fn the_default_negotiates_normally() {
        assert_eq!(Http2PriorKnowledge::default(), Http2PriorKnowledge::Off);
        assert!(!Http2PriorKnowledge::default().is_on());
    }

    #[test]
    fn the_on_variant_reports_prior_knowledge() {
        assert!(Http2PriorKnowledge::On.is_on());
    }
}
