use derive_more::{AsRef, Display, From};
use std::error::Error;

/// One link of an error's `source()` chain, rendered.
///
/// The chain is walked once, where the failure is turned into a diagnostic, and each link is kept as
/// its own value so the renderer can lay the chain out as rows and the dedup key can distinguish two
/// failures that share a headline but not a root cause.
#[derive(AsRef, Clone, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
#[as_ref(str)]
pub struct Cause(Box<str>);

impl Cause {
    /// Builds a cause from its rendered text.
    #[must_use]
    pub fn new(cause: impl Into<Box<str>>) -> Cause {
        Cause(cause.into())
    }

    /// The cause's text.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

/// Walks an error's `source()` chain, rendering each link in order.
///
/// This is the workspace's only chain walk: a failure is turned into a diagnostic once, and every
/// later consumer reads the rendered links rather than re-walking a chain that may no longer exist.
#[must_use]
pub fn causes_of(error: &dyn Error) -> Vec<Cause> {
    let mut causes = Vec::new();
    let mut source = error.source();
    while let Some(cause) = source {
        causes.push(Cause::new(cause.to_string()));
        source = cause.source();
    }
    causes
}

#[cfg(test)]
mod tests {
    use crate::cause::{Cause, causes_of};
    use std::{
        error::Error,
        fmt::{Display, Formatter, Result as FmtResult},
    };

    /// An error with an explicit chain, since `std::io::Error` deliberately skips a nesting level in
    /// its own `source()` and so cannot stand in for a layered failure.
    #[derive(Debug)]
    struct Layered {
        message: &'static str,
        source: Option<Box<Layered>>,
    }

    impl Display for Layered {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
            formatter.write_str(self.message)
        }
    }

    impl Error for Layered {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.source.as_ref().map(|source| source.as_ref() as &(dyn Error + 'static))
        }
    }

    fn layered(messages: &[&'static str]) -> Layered {
        let mut chain: Option<Box<Layered>> = None;
        for message in messages.iter().rev() {
            chain = Some(Box::new(Layered { message, source: chain }));
        }
        *chain.expect("at least one message")
    }

    #[test]
    fn a_chain_renders_every_link_below_the_top_error() {
        let causes = causes_of(&layered(&["POST failed", "TLS handshake failed", "connection reset"]));

        assert_eq!(causes.len(), 2);
        assert_eq!(causes[0], Cause::new("TLS handshake failed"));
        assert_eq!(causes[1], Cause::new("connection reset"));
    }

    #[test]
    fn an_error_without_a_source_yields_no_causes() {
        assert!(causes_of(&layered(&["standalone"])).is_empty());
    }
}
