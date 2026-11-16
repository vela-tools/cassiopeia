/// Whether a cycle checks its server for `MultiDatastreams` its input reads none of.
///
/// A run checks once, in the cycle that starts first, so a scheduled run asks a server at most once
/// whatever its schedule, and a cycle whose input reads `MultiDatastreams` never checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnreadCheck {
    /// The cycle checks.
    Due,
    /// The run has checked already.
    Done,
}
