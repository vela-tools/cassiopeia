use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// How a file writer frames the entities it emits into each per-type file.
///
/// Framing is orthogonal to whether an `@context` is attached: the two together pick the file
/// extension (`json`/`jsonld` for [`Array`](FileFraming::Array),
/// `jsonl`/`ndjsonld` for [`LineDelimited`](FileFraming::LineDelimited)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum FileFraming {
    /// A single pretty-printed JSON array holding every entity of the type.
    #[default]
    Array,
    /// One compact entity per line, with no surrounding array: append-friendly and re-indent-free.
    LineDelimited,
}

#[cfg(test)]
mod tests {
    use crate::file_framing::FileFraming;

    #[test]
    fn every_framing_round_trips_through_its_kebab_case_token() {
        for (framing, token) in [(FileFraming::Array, r#""array""#), (FileFraming::LineDelimited, r#""line-delimited""#)] {
            assert_eq!(serde_json::to_string(&framing).unwrap(), token);
            assert_eq!(serde_json::from_str::<FileFraming>(token).unwrap(), framing);
        }
    }

    #[test]
    fn framing_defaults_to_a_json_array() {
        assert_eq!(FileFraming::default(), FileFraming::Array);
    }
}
