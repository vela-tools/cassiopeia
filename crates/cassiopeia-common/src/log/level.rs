use serde::{Deserialize, Serialize};

/// The lowest severity a log record must have to be emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// The most verbose level, tracing individual events.
    Trace,
    /// Diagnostic detail useful while debugging.
    Debug,
    /// Ordinary progress information.
    Info,
    /// A recoverable problem worth surfacing.
    Warn,
    /// A failure that stopped an operation.
    Error,
}

#[cfg(test)]
mod tests {
    use crate::log::level::LogLevel;

    #[test]
    fn every_level_round_trips_through_its_lowercase_token() {
        for (level, token) in [
            (LogLevel::Trace, r#""trace""#),
            (LogLevel::Debug, r#""debug""#),
            (LogLevel::Info, r#""info""#),
            (LogLevel::Warn, r#""warn""#),
            (LogLevel::Error, r#""error""#),
        ] {
            assert_eq!(serde_json::to_string(&level).unwrap(), token);
            assert_eq!(serde_json::from_str::<LogLevel>(token).unwrap(), level);
        }
    }
}
