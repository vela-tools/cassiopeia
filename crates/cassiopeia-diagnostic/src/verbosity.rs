/// How much of a diagnostic is rendered.
///
/// Fixed for the process at reporter construction, so no level is threaded through a call graph and
/// no global is consulted on the rendering path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Verbosity {
    /// A headline plus at most two indented lines.
    Concise,
    /// The full cause chain and every context field.
    Full,
}

#[cfg(test)]
mod tests {
    use crate::verbosity::Verbosity;

    #[test]
    fn the_two_verbosities_are_distinct() {
        assert_ne!(Verbosity::Concise, Verbosity::Full);
    }
}
