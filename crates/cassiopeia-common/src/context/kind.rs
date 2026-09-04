use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// The discriminant exposed to the CLI via `--context`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ContextType {
    /// Do not attach any `@context`.
    None,
    /// Resolve the context URL from locally downloaded Smart Data Models.
    Default,
    /// Use an explicit context URL.
    Url,
    /// Inline a local `.jsonld` file's `@context` into every entity.
    Local,
}

#[cfg(test)]
mod tests {
    use crate::context::kind::ContextType;

    #[test]
    fn every_type_round_trips_through_its_kebab_case_token() {
        for (context_type, token) in [
            (ContextType::None, r#""none""#),
            (ContextType::Default, r#""default""#),
            (ContextType::Url, r#""url""#),
            (ContextType::Local, r#""local""#),
        ] {
            assert_eq!(serde_json::to_string(&context_type).unwrap(), token);
            assert_eq!(serde_json::from_str::<ContextType>(token).unwrap(), context_type);
        }
    }
}
