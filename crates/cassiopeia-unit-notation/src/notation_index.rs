use crate::{
    candidates::Candidates,
    notation_key::{NameKey, SymbolKey},
    notation_sources::{name_candidates, qudt_unit_candidates, symbol_candidates},
    qudt_unit_iri::QudtUnitId,
    ruling::{NAME_RULINGS, Ruling, SYMBOL_RULINGS},
    supplement::{SUPPLEMENTARY_NAMES, SUPPLEMENTARY_QUDT_UNITS, SUPPLEMENTARY_SYMBOLS, offer_supplements},
};
use cefact_units::UnitCode;
use std::{collections::HashMap, hash::Hash, sync::LazyLock};

/// The code every resolvable notation denotes, one table per kind of notation.
#[derive(Debug)]
pub(crate) struct NotationIndex {
    /// QUDT units by local name.
    qudt_units: HashMap<&'static str, UnitCode>,
    /// Symbols, UCUM codes, and UDUNITS codes.
    symbols: HashMap<SymbolKey, UnitCode>,
    /// Unit names.
    names: HashMap<NameKey, UnitCode>,
}

/// The index every resolution reads, built from the sources on first use.
static SHARED: LazyLock<NotationIndex> = LazyLock::new(NotationIndex::build);

impl NotationIndex {
    /// The index every resolution reads.
    pub(crate) fn shared() -> &'static NotationIndex {
        &SHARED
    }

    /// Settles every notation the sources and supplements offer.
    fn build() -> NotationIndex {
        NotationIndex {
            qudt_units: offered_qudt_units().settle(&HashMap::new()).resolved,
            symbols: offered_symbols().settle(&keyed_rulings(&SYMBOL_RULINGS, SymbolKey::of)).resolved,
            names: offered_names().settle(&keyed_rulings(&NAME_RULINGS, NameKey::of)).resolved,
        }
    }

    /// The code the QUDT unit `id` denotes.
    pub(crate) fn qudt_unit(&self, id: &QudtUnitId) -> Option<UnitCode> {
        self.qudt_units.get(id.as_str()).copied()
    }

    /// The code the symbol, UCUM code, or UDUNITS code `key` denotes.
    pub(crate) fn symbol(&self, key: &SymbolKey) -> Option<UnitCode> {
        self.symbols.get(key).copied()
    }

    /// The code the unit name `key` denotes.
    pub(crate) fn name(&self, key: &NameKey) -> Option<UnitCode> {
        self.names.get(key).copied()
    }
}

/// Every QUDT unit the sources and supplements offer.
fn offered_qudt_units() -> Candidates<&'static str> {
    let mut candidates = qudt_unit_candidates();
    offer_supplements(&mut candidates, &SUPPLEMENTARY_QUDT_UNITS, Some);
    candidates
}

/// Every symbol, UCUM code, and UDUNITS code the sources and supplements offer.
fn offered_symbols() -> Candidates<SymbolKey> {
    let mut candidates = symbol_candidates();
    offer_supplements(&mut candidates, &SUPPLEMENTARY_SYMBOLS, SymbolKey::of);
    candidates
}

/// Every unit name the sources and supplements offer.
fn offered_names() -> Candidates<NameKey> {
    let mut candidates = name_candidates();
    offer_supplements(&mut candidates, &SUPPLEMENTARY_NAMES, NameKey::of);
    candidates
}

/// `rulings` keyed as the notations they rule are.
fn keyed_rulings<Key: Eq + Hash>(rulings: &[(&str, Ruling)], key_of: fn(&str) -> Option<Key>) -> HashMap<Key, Ruling> {
    rulings.iter().filter_map(|&(notation, ruling)| Some((key_of(notation)?, ruling))).collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        notation_index::{NotationIndex, keyed_rulings, offered_names, offered_qudt_units, offered_symbols},
        notation_key::{NameKey, SymbolKey},
        notation_sources::{name_candidates, qudt_unit_candidates, symbol_candidates},
        ruling::{NAME_RULINGS, Ruling, SYMBOL_RULINGS},
        supplement::{SUPPLEMENTARY_NAMES, SUPPLEMENTARY_QUDT_UNITS, SUPPLEMENTARY_SYMBOLS},
    };
    use cefact_units::Status;
    use pretty_assertions::assert_eq;
    use std::collections::{BTreeMap, BTreeSet, HashMap};

    #[test]
    fn no_two_rulings_fold_to_one_notation() {
        assert_eq!(keyed_rulings(&SYMBOL_RULINGS, SymbolKey::of).len(), SYMBOL_RULINGS.len());
        assert_eq!(keyed_rulings(&NAME_RULINGS, NameKey::of).len(), NAME_RULINGS.len());
    }

    #[test]
    fn every_contested_symbol_has_a_ruling_and_every_symbol_ruling_a_contest() {
        let contested = offered_symbols().settle(&HashMap::new()).contested;
        let ruled: BTreeSet<SymbolKey> = keyed_rulings(&SYMBOL_RULINGS, SymbolKey::of).into_keys().collect();

        assert_eq!(contested.into_keys().collect::<BTreeSet<_>>(), ruled);
    }

    #[test]
    fn every_contested_name_has_a_ruling_and_every_name_ruling_a_contest() {
        let contested = offered_names().settle(&HashMap::new()).contested;
        let ruled: BTreeSet<NameKey> = keyed_rulings(&NAME_RULINGS, NameKey::of).into_keys().collect();

        assert_eq!(contested.into_keys().collect::<BTreeSet<_>>(), ruled);
    }

    #[test]
    fn every_ruled_code_is_one_of_the_notations_candidates() {
        let symbols = offered_symbols().settle(&HashMap::new()).contested;
        for (key, ruling) in keyed_rulings(&SYMBOL_RULINGS, SymbolKey::of) {
            if let Ruling::Code(code) = ruling {
                assert!(symbols[&key].contains(&code), "{key:?} is ruled {code:?}, which is not a candidate");
            }
        }
        let names = offered_names().settle(&HashMap::new()).contested;
        for (key, ruling) in keyed_rulings(&NAME_RULINGS, NameKey::of) {
            if let Ruling::Code(code) = ruling {
                assert!(names[&key].contains(&code), "{key:?} is ruled {code:?}, which is not a candidate");
            }
        }
    }

    #[test]
    fn no_qudt_unit_reads_as_two_current_codes() {
        assert_eq!(offered_qudt_units().settle(&HashMap::new()).contested, BTreeMap::new());
    }

    #[test]
    fn no_source_lists_a_supplementary_notation() {
        let symbols = symbol_candidates().settle(&HashMap::new());
        for (symbol, _) in SUPPLEMENTARY_SYMBOLS {
            let key = SymbolKey::of(symbol).unwrap();
            assert!(
                !symbols.resolved.contains_key(&key) && !symbols.contested.contains_key(&key),
                "a source lists {symbol}"
            );
        }
        let names = name_candidates().settle(&HashMap::new());
        for (name, _) in SUPPLEMENTARY_NAMES {
            let key = NameKey::of(name).unwrap();
            assert!(
                !names.resolved.contains_key(&key) && !names.contested.contains_key(&key),
                "a source lists {name}"
            );
        }
        let units = qudt_unit_candidates().settle(&HashMap::new());
        for (unit, _) in SUPPLEMENTARY_QUDT_UNITS {
            assert!(
                !units.resolved.contains_key(unit) && !units.contested.contains_key(unit),
                "QUDT gives {unit} a code"
            );
        }
    }

    #[test]
    fn no_resolved_code_is_deleted() {
        let index = NotationIndex::build();
        let codes: BTreeSet<_> = index.qudt_units.values().chain(index.symbols.values()).chain(index.names.values()).collect();

        assert!(codes.iter().all(|code| code.status() != Status::Deleted));
    }

    #[test]
    fn the_index_holds_every_kind_of_notation() {
        let index = NotationIndex::build();

        assert!(index.qudt_units.len() > 1_000, "{} QUDT units", index.qudt_units.len());
        assert!(index.symbols.len() > 2_000, "{} symbols", index.symbols.len());
        assert!(index.names.len() > 2_000, "{} names", index.names.len());
    }
}
