use crate::{generator_error::GeneratorError, qudt_release::QudtRelease, qudt_unit_name::QudtUnitName, unit_notations::UnitNotations};
use oxrdf::{NamedNodeRef, NamedOrBlankNode, Term};
use oxttl::TurtleParser;
use std::collections::{BTreeMap, BTreeSet};

/// `owl:versionIRI`, which names the release of the vocabulary's ontology.
const VERSION_IRI: NamedNodeRef<'_> = NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#versionIRI");

/// The units of one release of the QUDT unit vocabulary, with what each states about its notations.
#[derive(Debug)]
pub struct QudtVocabulary {
    /// The release the vocabulary belongs to.
    pub release: QudtRelease,
    /// Every unit with a literal the table reads, by local name.
    units: BTreeMap<QudtUnitName, UnitNotations>,
}

impl QudtVocabulary {
    /// The vocabulary the Turtle document `turtle` holds.
    ///
    /// # Errors
    /// [`GeneratorError::Syntax`] when `turtle` is not well-formed Turtle, and [`GeneratorError::NoRelease`]
    /// when it states no release.
    pub fn parse(turtle: &[u8]) -> Result<QudtVocabulary, GeneratorError> {
        let mut release = None;
        let mut units: BTreeMap<QudtUnitName, UnitNotations> = BTreeMap::new();
        for triple in TurtleParser::new().for_slice(turtle) {
            let triple = triple?;
            let predicate = triple.predicate.as_ref();
            match (&triple.subject, &triple.object) {
                (NamedOrBlankNode::NamedNode(_), Term::NamedNode(version)) if predicate == VERSION_IRI => {
                    release = release.or_else(|| QudtRelease::from_version_iri(version.as_str()));
                }
                (NamedOrBlankNode::NamedNode(subject), Term::Literal(literal)) => {
                    if let Some(unit) = QudtUnitName::from_iri(subject.as_str()) {
                        units.entry(unit).or_default().record(predicate, literal);
                    }
                }
                (NamedOrBlankNode::NamedNode(_), Term::NamedNode(_) | Term::BlankNode(_))
                | (NamedOrBlankNode::BlankNode(_), Term::NamedNode(_) | Term::BlankNode(_) | Term::Literal(_)) => {}
            }
        }
        Ok(QudtVocabulary {
            release: release.ok_or(GeneratorError::NoRelease)?,
            units,
        })
    }

    /// Every unit that carries a code Recommendation No. 20 lists, in order of local name.
    pub fn units(&self) -> impl Iterator<Item = (&QudtUnitName, &UnitNotations)> {
        self.units.iter().filter(|(_, notations)| !notations.codes.is_empty())
    }

    /// Every unit that carries a code no revision of Recommendation No. 20 lists, with those codes.
    pub fn unknown_codes(&self) -> impl Iterator<Item = (&QudtUnitName, &BTreeSet<String>)> {
        self.units
            .iter()
            .filter(|(_, notations)| !notations.unknown_codes.is_empty())
            .map(|(unit, notations)| (unit, &notations.unknown_codes))
    }
}

#[cfg(test)]
mod tests {
    use crate::{generator_error::GeneratorError, qudt_vocabulary::QudtVocabulary};
    use cefact_units::UnitCode;

    const VOCABULARY: &str = r##"
        @prefix owl: <http://www.w3.org/2002/07/owl#> .
        @prefix qudt: <http://qudt.org/schema/qudt/> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        @prefix unit: <http://qudt.org/vocab/unit/> .

        <http://qudt.org/3.5.3/vocab/unit> a owl:Ontology ; owl:versionIRI <http://qudt.org/3.5.3/vocab/unit> .

        unit:MicroGM-PER-M3 a qudt:Unit ;
            qudt:symbol "μg/m³" ;
            qudt:ucumCode "ug.m-3"^^qudt:UCUMcs ;
            qudt:uneceCommonCode "GQ" ;
            qudt:hasFactorUnit [ qudt:hasUnit unit:M ] ;
            rdfs:label "Microgram per Cubic Metre"@en .

        unit:NUM a qudt:Unit ; qudt:symbol "#" ; rdfs:label "Number"@en .

        unit:N-M-PER-M2 a qudt:Unit ; qudt:uneceCommonCode "H86" .
    "##;

    #[test]
    fn units_with_a_listed_code_are_read_with_their_notations() {
        let vocabulary = QudtVocabulary::parse(VOCABULARY.as_bytes()).unwrap();
        let units: Vec<_> = vocabulary
            .units()
            .map(|(unit, notations)| (unit.as_str(), notations.codes.iter().copied().collect::<Vec<_>>()))
            .collect();

        assert_eq!(vocabulary.release.to_string(), "3.5.3");
        assert_eq!(units, [("MicroGM-PER-M3", vec![UnitCode::Gq])]);
    }

    #[test]
    fn a_unit_whose_code_no_revision_lists_is_reported() {
        let vocabulary = QudtVocabulary::parse(VOCABULARY.as_bytes()).unwrap();
        let unknown: Vec<_> = vocabulary.unknown_codes().map(|(unit, codes)| (unit.as_str(), codes.len())).collect();

        assert_eq!(unknown, [("N-M-PER-M2", 1)]);
    }

    #[test]
    fn a_vocabulary_without_a_release_is_refused() {
        let error = QudtVocabulary::parse(b"<http://qudt.org/vocab/unit/A> <http://qudt.org/schema/qudt/symbol> \"A\" .").unwrap_err();

        assert!(matches!(error, GeneratorError::NoRelease));
    }

    #[test]
    fn malformed_turtle_is_refused() {
        let error = QudtVocabulary::parse(b"unit:A qudt:symbol").unwrap_err();

        assert!(matches!(error, GeneratorError::Syntax(_)));
    }
}
