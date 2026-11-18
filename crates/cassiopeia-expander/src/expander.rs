use crate::{error::ExpanderError, expansion_losses::ExpansionLosses};
use cassiopeia_ir::{fragment::Fragment, mapped::Mapped, record::Record, source_position::SourcePosition};

/// Expands raw records into mapped NGSI-LD fragments.
///
/// This is the contract for record expansion: given a single [`Record`], an implementation
/// produces one [`Mapped<Fragment>`] per entity the record describes, according to its mapping
/// configuration.
pub trait Expander: Send + Sync {
    /// Expands a single record into the entities it describes, each fragment carrying the record's
    /// `position`.
    ///
    /// Each entity comes back as its own result: its fragment, or the [`ExpanderError`] that keeps
    /// it from being written, such as an identity or scope that fails to resolve into a valid URN or
    /// scope. No entity of a record needs another to exist, so one that fails costs only itself. A
    /// record that no mapping is bound to describes no entity, and comes back as that one failure.
    ///
    /// What an entity loses while it is still written is recorded in `losses`, so the stage that
    /// owns the batch can name it. An entity expiry that resolves to text no instant reads is
    /// dropped from the fragment, and a relationship whose source, or whose synthetic entity's
    /// identity, fails to render is left out with that synthetic entity.
    fn expand(&self, record: Record, position: SourcePosition, losses: &ExpansionLosses) -> Vec<Result<Mapped<Fragment>, ExpanderError>>;

    /// Expands a batch of consecutive records, the first of which stands at `first` in the source.
    ///
    /// The results come back in the order of `records`, each record's entities in the order
    /// [`expand`](Expander::expand) gives them. The default implementation processes records
    /// sequentially; implementations may override it to provide an optimized execution strategy.
    fn expand_batch(&self, records: Vec<Record>, first: SourcePosition, losses: &ExpansionLosses) -> Vec<Result<Mapped<Fragment>, ExpanderError>> {
        records
            .into_iter()
            .enumerate()
            .flat_map(|(offset, record)| self.expand(record, first.advanced(offset), losses))
            .collect()
    }
}
