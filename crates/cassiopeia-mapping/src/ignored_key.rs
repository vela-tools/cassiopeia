use crate::{declaration_site::DeclarationSite, document_key::DocumentKey};
use getset::Getters;
use strum::{Display, IntoStaticStr};

/// A key the mapping format defines that has no effect where a document declares it.
#[derive(Clone, Copy, Debug, Display, Eq, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "camelCase")]
pub enum IneffectiveKey {
    /// A target's `sourceField`, which nothing reads: a relationship's object is the entity its
    /// `source` names.
    SourceField,
    /// A target's `idPattern`, which nothing reads, for the same reason as `sourceField`.
    IdPattern,
    /// A `syntheticEntity` on an attribute that no entity is written from: anything but a top-level
    /// Relationship or `ListRelationship` of the document, which links the main entity to the one it
    /// writes.
    SyntheticEntity,
}

/// Why a key a mapping document declares is ignored.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IgnoredKeyKind {
    /// A key the declaration does not read.
    Unknown(DocumentKey),
    /// A key the mapping format defines, which has no effect where it is declared.
    WithoutEffect(IneffectiveKey),
}

/// A key a mapping document declares that loading the mapping ignores, so whatever it declares has
/// no effect on the entities the mapping writes.
///
/// The mapping still loads: the key is reported, once, before any record is read.
#[derive(Clone, Debug, Eq, Getters, PartialEq)]
#[getset(get = "pub")]
pub struct IgnoredKey {
    /// The declaration holding the key.
    site: DeclarationSite,
    /// Which key it is, and why it is ignored.
    kind: IgnoredKeyKind,
}

impl IgnoredKey {
    /// The key `kind` names, held by the declaration at `site`.
    #[must_use]
    pub const fn new(site: DeclarationSite, kind: IgnoredKeyKind) -> IgnoredKey {
        IgnoredKey { site, kind }
    }
}

#[cfg(test)]
mod tests {
    use crate::ignored_key::IneffectiveKey;
    use pretty_assertions::assert_eq;

    #[test]
    fn each_ineffective_key_renders_as_the_document_spells_it() {
        assert_eq!(IneffectiveKey::SourceField.to_string(), "sourceField");
        assert_eq!(<&str>::from(IneffectiveKey::IdPattern), "idPattern");
        assert_eq!(IneffectiveKey::SyntheticEntity.to_string(), "syntheticEntity");
    }
}
