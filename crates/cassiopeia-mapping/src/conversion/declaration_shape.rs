use derive_more::Display;

/// Where a declaration that converts one value takes that value from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
pub enum ValueOrigin {
    /// Its `source`, one template or several.
    #[display("read from a `source`")]
    Sourced,
    /// The object its nested `mappings` assemble, member by member.
    #[display("built from nested `mappings`")]
    Assembled,
}

/// What a declaration is built from, which narrows the transformations it can take.
///
/// A `languageMap` is read only by a `LanguageProperty`, whose `languageMap` entries each convert
/// one localized string (ETSI GS CIM 009 v1.9.1 clause 4.5.18), so both the declaration holding the
/// map and each of its entries are shapes of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
pub enum DeclarationShape {
    /// A declaration converting one value, read from its `source` or assembled from its nested
    /// `mappings`.
    #[display("{_0}")]
    Value(ValueOrigin),
    /// A declaration holding a `languageMap` of per-language entries.
    #[display("built from a `languageMap`")]
    LanguageMap,
    /// One entry of a `languageMap`.
    #[display("a `languageMap` entry")]
    LanguageEntry,
}

impl DeclarationShape {
    /// Every shape a declaration can take.
    pub fn iter() -> impl Iterator<Item = DeclarationShape> {
        [
            DeclarationShape::Value(ValueOrigin::Sourced),
            DeclarationShape::Value(ValueOrigin::Assembled),
            DeclarationShape::LanguageMap,
            DeclarationShape::LanguageEntry,
        ]
        .into_iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::conversion::declaration_shape::{DeclarationShape, ValueOrigin};
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    #[test]
    fn each_shape_displays_as_the_phrase_a_load_error_names_it_by() {
        assert_eq!(DeclarationShape::Value(ValueOrigin::Sourced).to_string(), "read from a `source`");
        assert_eq!(DeclarationShape::Value(ValueOrigin::Assembled).to_string(), "built from nested `mappings`");
        assert_eq!(DeclarationShape::LanguageMap.to_string(), "built from a `languageMap`");
        assert_eq!(DeclarationShape::LanguageEntry.to_string(), "a `languageMap` entry");
    }

    #[test]
    fn every_shape_is_listed_once() {
        assert_eq!(DeclarationShape::iter().collect::<HashSet<_>>().len(), DeclarationShape::iter().count());
        assert_eq!(DeclarationShape::iter().count(), 4);
    }
}
