use strum::{Display, EnumCount, EnumIter};

/// Why a key a mapping document declares has no effect, found when the mapping loads.
///
/// Each code names the mapping format it belongs to, since what a key means, and whether it means
/// anything, is fixed by that format.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MappingCode {
    /// An attribute, a sub-attribute, a member under `mappings`, a `languageMap` entry, an instance
    /// or a target of a v4 mapping declares a key it does not read, so the key has no effect.
    #[strum(serialize = "v4-unknown-key")]
    V4UnknownKey,
    /// A v4 mapping declares a key that has no effect where it is declared: a target's
    /// `sourceField` or `idPattern`, which nothing reads, or a `syntheticEntity` on an attribute
    /// that no entity is written from.
    #[strum(serialize = "v4-key-without-effect")]
    V4KeyWithoutEffect,
}

#[cfg(test)]
mod tests {
    use crate::code::{diagnostic_code::DiagnosticCode, mapping_code::MappingCode};
    use pretty_assertions::assert_eq;

    #[test]
    fn each_reason_a_mapping_key_has_no_effect_renders_its_own_code() {
        assert_eq!(DiagnosticCode::Mapping(MappingCode::V4UnknownKey).to_string(), "mapping-v4-unknown-key");
        assert_eq!(
            DiagnosticCode::Mapping(MappingCode::V4KeyWithoutEffect).to_string(),
            "mapping-v4-key-without-effect"
        );
    }
}
