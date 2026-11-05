use crate::transformation::Transformation;

/// Why a declaration's type, shape, and transformation resolve to no conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionRefusal {
    /// The declared transformation can never produce the value the declaration's type holds from
    /// what its shape reads.
    Transformation(Transformation),
    /// The declaration's type can never be built from what its shape reads, whatever the
    /// transformation: a `VocabProperty` or a relationship assembled from nested `mappings`, or any
    /// type but a `LanguageProperty` holding a `languageMap`.
    Shape,
}
