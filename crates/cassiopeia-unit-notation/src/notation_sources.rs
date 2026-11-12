use crate::{
    candidates::Candidates,
    notation_key::{NameKey, SymbolKey},
    qudt::{correction::codes_read, generated::QUDT_UNITS, qudt_unit::QudtUnit},
};
use cefact_units::UnitCode;
use std::hash::Hash;

/// Every QUDT unit, by its local name, offered for the codes Cassiopeia reads it as.
pub(crate) fn qudt_unit_candidates() -> Candidates<&'static str> {
    let mut candidates = Candidates::new();
    for unit in &QUDT_UNITS {
        for &code in codes_read(unit) {
            candidates.offer(unit.id, code);
        }
    }
    candidates
}

/// Every symbol, UCUM code, and UDUNITS code a source lists, offered for each code it gives it.
///
/// Recommendation No. 20 sometimes lists two symbols in one entry, as `% or pct`; each is offered alone.
pub(crate) fn symbol_candidates() -> Candidates<SymbolKey> {
    let mut candidates = Candidates::new();
    for &code in UnitCode::ALL {
        let symbols = code.info().symbol.into_iter().flat_map(|symbol| symbol.split(" or "));
        offer_each(&mut candidates, symbols, SymbolKey::of, code);
    }
    for unit in &QUDT_UNITS {
        offer_for_qudt_unit(&mut candidates, unit, unit.symbols, SymbolKey::of);
    }
    candidates
}

/// Every unit name a source lists, offered for each code it gives it: the name Recommendation No. 20
/// gives each code, and the English labels of each QUDT unit.
pub(crate) fn name_candidates() -> Candidates<NameKey> {
    let mut candidates = Candidates::new();
    for &code in UnitCode::ALL {
        offer_each(&mut candidates, [code.info().name], NameKey::of, code);
    }
    for unit in &QUDT_UNITS {
        offer_for_qudt_unit(&mut candidates, unit, unit.labels, NameKey::of);
    }
    candidates
}

/// Offers each of `unit`'s `notations` for every code Cassiopeia reads `unit` as.
fn offer_for_qudt_unit<Key: Ord + Hash + Clone>(candidates: &mut Candidates<Key>, unit: &QudtUnit, notations: &[&str], key_of: fn(&str) -> Option<Key>) {
    for &code in codes_read(unit) {
        offer_each(candidates, notations.iter().copied(), key_of, code);
    }
}

/// Offers the key of each of `notations` that has one for `code`.
fn offer_each<'a, Key: Ord + Hash + Clone>(
    candidates: &mut Candidates<Key>,
    notations: impl IntoIterator<Item = &'a str>,
    key_of: fn(&str) -> Option<Key>,
    code: UnitCode,
) {
    for key in notations.into_iter().filter_map(key_of) {
        candidates.offer(key, code);
    }
}
