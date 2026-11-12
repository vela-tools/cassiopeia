use derive_more::Display;

/// A QUDT release, such as `3.5.3`, as its unit vocabulary's version IRI names it.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub struct QudtRelease(String);

impl QudtRelease {
    /// The release `version_iri` names, when it has the form `http://qudt.org/<release>/vocab/unit`.
    pub fn from_version_iri(version_iri: &str) -> Option<QudtRelease> {
        let release = version_iri.strip_prefix("http://qudt.org/")?.strip_suffix("/vocab/unit")?;
        let well_formed = !release.is_empty()
            && release
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
        well_formed.then(|| QudtRelease(release.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use crate::qudt_release::QudtRelease;

    #[test]
    fn a_unit_vocabulary_version_iri_names_its_release() {
        let release = QudtRelease::from_version_iri("http://qudt.org/3.5.3/vocab/unit").map(|release| release.to_string());

        assert_eq!(release.as_deref(), Some("3.5.3"));
    }

    #[test]
    fn another_iri_names_no_release() {
        assert_eq!(QudtRelease::from_version_iri("http://qudt.org/3.5.3/vocab/quantitykind"), None);
        assert_eq!(QudtRelease::from_version_iri("http://qudt.org/latest/vocab/unit"), None);
        assert_eq!(QudtRelease::from_version_iri("http://qudt.org//vocab/unit"), None);
    }
}
