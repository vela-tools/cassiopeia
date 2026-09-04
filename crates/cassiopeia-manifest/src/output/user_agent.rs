use derive_more::{AsRef, Display, From, FromStr};
use serde::{Deserialize, Serialize};

/// The value sent in the HTTP `User-Agent` header when entities are pushed to a broker.
///
/// A run's user-agent is chosen by the manifest or the command line; there is deliberately no
/// `Default` here. The fallback identifier is a build-time value the binary supplies (the release
/// version and commit), not a constant this crate could know, so callers resolve the default at the
/// composition root rather than through `Default::default()`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, FromStr, Serialize, Deserialize, From, AsRef)]
#[serde(transparent)]
pub struct UserAgent(#[as_ref(str)] String);

#[cfg(test)]
mod tests {
    use crate::output::user_agent::UserAgent;
    use std::str::FromStr;

    #[test]
    fn a_user_agent_displays_and_serializes_transparently() {
        let agent = UserAgent::from("cassiopeia/1.2.3".to_string());

        assert_eq!(agent.to_string(), "cassiopeia/1.2.3");
        assert_eq!(serde_json::to_string(&agent).unwrap(), r#""cassiopeia/1.2.3""#);
    }

    #[test]
    fn a_user_agent_parses_from_a_string_and_round_trips() {
        let agent = UserAgent::from_str("acme/0.1").unwrap();

        assert_eq!(<UserAgent as AsRef<str>>::as_ref(&agent), "acme/0.1");
        assert_eq!(serde_json::from_str::<UserAgent>(r#""acme/0.1""#).unwrap(), agent);
    }
}
