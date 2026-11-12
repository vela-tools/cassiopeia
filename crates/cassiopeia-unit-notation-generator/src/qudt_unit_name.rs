use derive_more::Display;

/// A unit's local name in the QUDT unit vocabulary: `MicroGM-PER-M3` in
/// `http://qudt.org/vocab/unit/MicroGM-PER-M3`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Display)]
pub struct QudtUnitName(String);

impl QudtUnitName {
    /// The namespace every unit IRI of the vocabulary starts with.
    const NAMESPACE: &str = "http://qudt.org/vocab/unit/";

    /// The local name of `iri`, when `iri` is a unit IRI of the vocabulary.
    pub fn from_iri(iri: &str) -> Option<QudtUnitName> {
        iri.strip_prefix(QudtUnitName::NAMESPACE)
            .filter(|name| !name.is_empty() && !name.contains('/'))
            .map(|name| QudtUnitName(name.to_owned()))
    }

    /// The local name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::qudt_unit_name::QudtUnitName;

    #[test]
    fn a_unit_iri_names_its_local_name() {
        let name = QudtUnitName::from_iri("http://qudt.org/vocab/unit/MicroGM-PER-M3");

        assert_eq!(name.as_ref().map(QudtUnitName::as_str), Some("MicroGM-PER-M3"));
    }

    #[test]
    fn an_iri_outside_the_unit_namespace_names_no_unit() {
        assert_eq!(QudtUnitName::from_iri("http://qudt.org/vocab/quantitykind/Mass"), None);
        assert_eq!(QudtUnitName::from_iri("http://qudt.org/vocab/unit/"), None);
        assert_eq!(QudtUnitName::from_iri("http://qudt.org/3.5.3/vocab/unit"), None);
    }
}
