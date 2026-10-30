use crate::error::ExpanderError;
use cassiopeia_ir::{fragment::Fragment, mapped::Mapped, record::Record};
use cassiopeia_unreadable_timestamps::unreadable_expiries::UnreadableExpiries;

/// Expands raw records into mapped NGSI-LD fragments.
///
/// This is the contract for record expansion: given a single [`Record`], an implementation
/// produces one or more [`Mapped<Fragment>`]s according to its mapping configuration.
pub trait Expander: Send + Sync {
    /// Expands a single record into one or more fragments.
    ///
    /// An entity expiry that resolves to text no instant reads is dropped from the fragments and
    /// recorded in `unreadable_expiries`, so the stage that owns the batch can name the loss; a
    /// record is never failed over one.
    ///
    /// # Errors
    ///
    /// Returns [`ExpanderError`] when the record's identity, scope, or a relationship target fails
    /// to resolve into a valid URN.
    fn expand(&self, record: Record, unreadable_expiries: &UnreadableExpiries) -> Result<Vec<Mapped<Fragment>>, ExpanderError>;

    /// Expands a batch of records.
    ///
    /// The default implementation processes records sequentially; implementations may override it
    /// to provide an optimized execution strategy.
    fn expand_batch(&self, records: Vec<Record>, unreadable_expiries: &UnreadableExpiries) -> Vec<Result<Vec<Mapped<Fragment>>, ExpanderError>> {
        records.into_iter().map(|record| self.expand(record, unreadable_expiries)).collect()
    }
}
