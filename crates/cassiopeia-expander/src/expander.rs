use crate::{error::ExpanderError, expansion_losses::ExpansionLosses};
use cassiopeia_ir::{fragment::Fragment, mapped::Mapped, record::Record};

/// Expands raw records into mapped NGSI-LD fragments.
///
/// This is the contract for record expansion: given a single [`Record`], an implementation
/// produces one or more [`Mapped<Fragment>`]s according to its mapping configuration.
pub trait Expander: Send + Sync {
    /// Expands a single record into one or more fragments.
    ///
    /// What the record loses while it is still expanded is recorded in `losses`, so the stage that
    /// owns the batch can name it; a record is never failed over one. An entity expiry that resolves
    /// to text no instant reads is dropped from the fragments, and a relationship whose source, or
    /// whose synthetic entity's identity, fails to render is left out with that synthetic entity.
    ///
    /// # Errors
    ///
    /// Returns [`ExpanderError`] when the record's identity or scope fails to resolve into a valid
    /// URN or scope, or a relationship target cannot be minted for a reason other than its template.
    fn expand(&self, record: Record, losses: &ExpansionLosses) -> Result<Vec<Mapped<Fragment>>, ExpanderError>;

    /// Expands a batch of records.
    ///
    /// The default implementation processes records sequentially; implementations may override it
    /// to provide an optimized execution strategy.
    fn expand_batch(&self, records: Vec<Record>, losses: &ExpansionLosses) -> Vec<Result<Vec<Mapped<Fragment>>, ExpanderError>> {
        records.into_iter().map(|record| self.expand(record, losses)).collect()
    }
}
