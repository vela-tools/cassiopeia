use serde::{Deserialize, Serialize};

/// How each log record is laid out on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// A dense single-line human-readable form.
    Compact,
    /// A multi-line human-readable form with fields spread out.
    Pretty,
    /// A structured JSON object per record, for shipping to a log store.
    Json,
}

#[cfg(test)]
mod tests {
    use crate::log::format::LogFormat;

    #[test]
    fn every_format_round_trips_through_its_lowercase_token() {
        for (format, token) in [
            (LogFormat::Compact, r#""compact""#),
            (LogFormat::Pretty, r#""pretty""#),
            (LogFormat::Json, r#""json""#),
        ] {
            assert_eq!(serde_json::to_string(&format).unwrap(), token);
            assert_eq!(serde_json::from_str::<LogFormat>(token).unwrap(), format);
        }
    }
}
