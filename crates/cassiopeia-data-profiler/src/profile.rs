use crate::metadata::FormatMetadata;
use cassiopeia_common::format::DataFormat;
use getset::Getters;
use mediatype::MediaType;
use serde::Serialize;

/// Result of a data profiling operation.
///
/// Records the format a detector recognised, the MIME type it maps to, and how
/// confident the detector was in the match. Detectors may also attach
/// format-specific metadata needed by later pipeline stages.
#[derive(Debug, Serialize, Clone, Getters)]
#[getset(get = "pub")]
pub struct Profile {
    /// The detected data format.
    format: DataFormat,
    /// MIME type corresponding to the detected format.
    mime_type: MediaType<'static>,
    /// Confidence score in the range `0.0` to `1.0`.
    confidence: f32,
    /// Format-specific metadata discovered during profiling, when available.
    metadata: Option<FormatMetadata>,
}

impl Profile {
    /// Builds a profile for a detected `format` with its `mime_type` and match `confidence`.
    #[must_use]
    pub const fn new(format: DataFormat, mime_type: MediaType<'static>, confidence: f32) -> Profile {
        Profile {
            format,
            mime_type,
            confidence,
            metadata: None,
        }
    }

    /// Adds format-specific metadata to this profile.
    #[must_use]
    pub fn with_metadata(mut self, metadata: FormatMetadata) -> Profile {
        self.metadata = Some(metadata);
        self
    }

    /// Consumes the profile and returns its owned format-specific metadata, if any.
    #[must_use]
    pub fn into_metadata(self) -> Option<FormatMetadata> {
        self.metadata
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        metadata::{CsvMetadata, FormatMetadata},
        profile::Profile,
    };
    use cassiopeia_common::format::DataFormat;
    use cassiopeia_csv_dialect::{
        character_encoding::CharacterEncoding,
        dialect::Dialect,
        dialect_byte::DialectByte,
        escape::Escape,
        header_rows::HeaderRows,
        line_terminator::LineTerminator,
        preamble::Preamble,
        quote::Quote,
        value_notation::ValueNotation,
    };
    use encoding_rs::UTF_8;
    use mediatype::media_type;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_profile_exposes_the_format_mime_and_confidence_it_was_built_with() {
        let profile = Profile::new(DataFormat::Csv, media_type!(TEXT / CSV), 0.9);

        assert_eq!(*profile.format(), DataFormat::Csv);
        assert_eq!(profile.mime_type().to_string(), "text/csv");
        assert!((*profile.confidence() - 0.9).abs() < f32::EPSILON);
        assert!(profile.metadata().is_none());
    }

    #[test]
    fn a_profile_serialises_its_mime_type_as_a_media_type_string() {
        let profile = Profile::new(DataFormat::Json, media_type!(APPLICATION / JSON), 0.8);
        let json = serde_json::to_value(&profile).unwrap();

        assert_eq!(json["mime_type"], "application/json");
    }

    #[test]
    fn a_profile_serialises_attached_format_metadata() {
        let dialect = Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::ONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
        };
        let profile = Profile::new(DataFormat::Csv, media_type!(TEXT / CSV), 0.9).with_metadata(FormatMetadata::Csv(CsvMetadata::new(dialect)));
        let json = serde_json::to_value(&profile).unwrap();

        assert_eq!(json["metadata"]["Csv"]["dialect"]["delimiter"], ";");
    }

    #[test]
    fn into_metadata_yields_the_attached_metadata_and_none_without_it() {
        let dialect = Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::ONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
        };
        let profile = Profile::new(DataFormat::Csv, media_type!(TEXT / CSV), 0.9).with_metadata(FormatMetadata::Csv(CsvMetadata::new(dialect)));

        assert!(matches!(profile.into_metadata(), Some(FormatMetadata::Csv(_))));
        assert!(Profile::new(DataFormat::Json, media_type!(APPLICATION / JSON), 0.8).into_metadata().is_none());
    }
}
