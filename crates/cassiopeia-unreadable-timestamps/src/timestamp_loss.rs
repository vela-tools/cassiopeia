/// Which timestamp of an attribute was lost because it read as no instant.
///
/// The three losses look the same to a mapping author (a source spelling Cassiopeia does not read),
/// but they cost different things: an attribute's own value is dropped entirely, while a qualifier
/// loss still publishes the attribute, without the qualifier that failed. The sink keys its records
/// by this alongside the attribute, so one attribute losing two different timestamps is two losses
/// named separately, each under the code that says what it cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimestampLoss {
    /// The attribute's own value, so no trace of the attribute is emitted at all.
    AttributeValue,
    /// The attribute's `observedAt` qualifier, so the value is emitted with nothing anchoring it in
    /// time (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2).
    ObservedAt,
    /// The attribute's `expiresAt` qualifier, so the value is emitted without the instant after
    /// which it ceases to hold (ETSI GS CIM 009 v1.9.1 clause 4.22).
    ExpiresAt,
}
