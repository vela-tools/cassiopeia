use url::Url;

/// The local name of a QUDT unit IRI: `MicroGM-PER-M3` in `https://qudt.org/vocab/unit/MicroGM-PER-M3`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QudtUnitId(String);

impl QudtUnitId {
    /// The local name `text` names, when `text` is a QUDT unit IRI.
    ///
    /// QUDT publishes its IRIs over `http`, and SensorThings servers such as RIVM Samen Meten write them
    /// over `https`, so either scheme is read. The path is `/vocab/unit/<name>`, with an optional
    /// trailing `/`; any other path, a query, a fragment, or a port names no unit.
    pub(crate) fn from_iri(text: &str) -> Option<QudtUnitId> {
        let iri = Url::parse(text).ok()?;
        let on_qudt = matches!(iri.scheme(), "http" | "https") && iri.host_str() == Some("qudt.org");
        if !on_qudt || iri.port().is_some() || iri.query().is_some() || iri.fragment().is_some() {
            return None;
        }
        let segments: Vec<&str> = iri.path_segments()?.collect();
        match segments.as_slice() {
            ["vocab", "unit", name] | ["vocab", "unit", name, ""] if !name.is_empty() => Some(QudtUnitId((*name).to_owned())),
            _ => None,
        }
    }

    /// The local name as text.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::qudt_unit_iri::QudtUnitId;
    use pretty_assertions::assert_eq;

    fn local_name(text: &str) -> Option<String> {
        QudtUnitId::from_iri(text).map(|id| id.as_str().to_owned())
    }

    #[test]
    fn a_unit_iri_over_http_or_https_names_its_local_name() {
        assert_eq!(local_name("http://qudt.org/vocab/unit/DEG_C"), Some("DEG_C".to_owned()));
        assert_eq!(local_name("https://qudt.org/vocab/unit/MicroGM-PER-M3"), Some("MicroGM-PER-M3".to_owned()));
    }

    #[test]
    fn a_trailing_slash_is_allowed() {
        assert_eq!(local_name("https://qudt.org/vocab/unit/DEG_C/"), Some("DEG_C".to_owned()));
    }

    #[test]
    fn the_local_name_keeps_its_case() {
        assert_eq!(local_name("https://qudt.org/vocab/unit/deg_c"), Some("deg_c".to_owned()));
    }

    #[test]
    fn an_iri_outside_the_unit_vocabulary_names_no_unit() {
        assert_eq!(local_name("https://qudt.org/vocab/quantitykind/Temperature"), None);
        assert_eq!(local_name("https://qudt.org/vocab/unit/"), None);
        assert_eq!(local_name("https://qudt.org/vocab/unit/DEG_C/extra"), None);
        assert_eq!(local_name("https://example.org/vocab/unit/DEG_C"), None);
        assert_eq!(local_name("ftp://qudt.org/vocab/unit/DEG_C"), None);
    }

    #[test]
    fn a_query_fragment_or_port_names_no_unit() {
        assert_eq!(local_name("https://qudt.org/vocab/unit/DEG_C?x=1"), None);
        assert_eq!(local_name("https://qudt.org/vocab/unit/DEG_C#x"), None);
        assert_eq!(local_name("https://qudt.org:8443/vocab/unit/DEG_C"), None);
    }

    #[test]
    fn a_ucum_page_anchor_and_plain_text_are_not_unit_iris() {
        assert_eq!(local_name("http://unitsofmeasure.org/ucum.html#para-30"), None);
        assert_eq!(local_name("ug/m3"), None);
    }
}
