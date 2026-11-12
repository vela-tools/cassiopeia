use crate::ruling::Ruling;
use cefact_units::{CodeList, Status, UnitCode};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    hash::Hash,
};

/// The codes each notation of one kind is offered for, gathered from every source before one is chosen.
#[derive(Debug)]
pub(crate) struct Candidates<Key> {
    /// Every notation offered, with each writable code a source offered it for.
    offers: BTreeMap<Key, BTreeSet<UnitCode>>,
}

impl<Key: Ord + Hash + Clone> Candidates<Key> {
    /// Candidates with nothing offered yet.
    pub(crate) const fn new() -> Candidates<Key> {
        Candidates { offers: BTreeMap::new() }
    }

    /// Offers `key` for `code`, unless `code` is not [writable](is_writable).
    pub(crate) fn offer(&mut self, key: Key, code: UnitCode) {
        if is_writable(code) {
            self.offers.entry(key).or_default().insert(code);
        }
    }

    /// Settles every notation on at most one code.
    ///
    /// A current code is preferred to a deprecated one offered for the same notation, so a deprecated code
    /// resolves only for a unit Recommendation No. 20 gives no current code. A Recommendation No. 20 code
    /// is then preferred to a Recommendation No. 21 package code: `piece` names the unit `H87`, not the
    /// package `XPP`. A notation with one candidate left resolves to it. One with several is contested and
    /// resolves as `rulings` says; a contested notation `rulings` leaves out resolves to nothing, since
    /// picking one of its units would be a guess.
    pub(crate) fn settle(self, rulings: &HashMap<Key, Ruling>) -> Settlement<Key> {
        let mut settlement = Settlement {
            resolved: HashMap::new(),
            contested: BTreeMap::new(),
        };
        for (key, codes) in self.offers {
            let codes = preferring(preferring(codes, is_current), is_recommendation_20);
            let mut remaining = codes.iter().copied();
            match (remaining.next(), remaining.next()) {
                (Some(code), None) => {
                    settlement.resolved.insert(key, code);
                }
                (Some(_), Some(_)) => {
                    if let Some(Ruling::Code(code)) = rulings.get(&key) {
                        // A key is cloned only for a contested notation, of which the sources hold a few dozen.
                        settlement.resolved.insert(key.clone(), *code);
                    }
                    settlement.contested.insert(key, codes);
                }
                (None, Some(_) | None) => {}
            }
        }
        settlement
    }
}

/// Whether `code` may be written in `unitCode`: a current or a deprecated code, never a deleted one.
///
/// Recommendation No. 20 still lists a deprecated code, only not for new data, and units in common use
/// such as the hectare (`HAR`) and the millimetre of mercury (`HN`) have no other code. A deleted code is
/// no longer part of the recommendation.
pub(crate) const fn is_writable(code: UnitCode) -> bool {
    match code.status() {
        Status::Current | Status::Deprecated => true,
        Status::Deleted => false,
    }
}

/// Whether `code` is current in the latest revision.
const fn is_current(code: UnitCode) -> bool {
    matches!(code.status(), Status::Current)
}

/// Whether `code` is drawn from Recommendation No. 20 rather than from the package codes of No. 21.
const fn is_recommendation_20(code: UnitCode) -> bool {
    matches!(code.code_list(), CodeList::Recommendation20)
}

/// `codes` narrowed to those `preferred` holds for, when it holds for any of them.
fn preferring(codes: BTreeSet<UnitCode>, preferred: fn(UnitCode) -> bool) -> BTreeSet<UnitCode> {
    if codes.iter().copied().any(preferred) {
        codes.into_iter().filter(|&code| preferred(code)).collect()
    } else {
        codes
    }
}

/// The outcome of settling the notations of one kind.
#[derive(Debug)]
pub(crate) struct Settlement<Key> {
    /// Every notation that resolves, with its code.
    pub(crate) resolved: HashMap<Key, UnitCode>,
    /// Every notation offered for several codes, with its candidates, whether or not a ruling settles it.
    pub(crate) contested: BTreeMap<Key, BTreeSet<UnitCode>>,
}

#[cfg(test)]
mod tests {
    use crate::{candidates::Candidates, ruling::Ruling};
    use cefact_units::UnitCode;
    use pretty_assertions::assert_eq;
    use std::collections::{BTreeSet, HashMap};

    #[test]
    fn a_notation_offered_for_one_code_resolves_to_it() {
        let mut candidates = Candidates::new();
        candidates.offer("kelvin", UnitCode::Kel);
        candidates.offer("kelvin", UnitCode::Kel);

        let settlement = candidates.settle(&HashMap::new());

        assert_eq!(settlement.resolved.get("kelvin"), Some(&UnitCode::Kel));
        assert!(settlement.contested.is_empty());
    }

    #[test]
    fn a_deleted_code_is_never_a_candidate() {
        let mut candidates = Candidates::new();
        candidates.offer("gram per kilogram", UnitCode::Gk);

        let settlement = candidates.settle(&HashMap::new());

        assert!(settlement.resolved.is_empty());
        assert!(settlement.contested.is_empty());
    }

    #[test]
    fn a_deprecated_code_alone_resolves() {
        let mut candidates = Candidates::new();
        candidates.offer("hectare", UnitCode::Har);

        assert_eq!(candidates.settle(&HashMap::new()).resolved.get("hectare"), Some(&UnitCode::Har));
    }

    #[test]
    fn a_current_code_wins_over_a_deprecated_one() {
        let mut candidates = Candidates::new();
        candidates.offer("Us Liquid Quart", UnitCode::Qt);
        candidates.offer("Us Liquid Quart", UnitCode::Qtl);

        let settlement = candidates.settle(&HashMap::new());

        assert_eq!(settlement.resolved.get("Us Liquid Quart"), Some(&UnitCode::Qtl));
        assert!(settlement.contested.is_empty());
    }

    #[test]
    fn a_recommendation_20_code_wins_over_a_recommendation_21_package_code() {
        let mut candidates = Candidates::new();
        candidates.offer("piece", UnitCode::H87);
        candidates.offer("piece", UnitCode::Xpp);

        let settlement = candidates.settle(&HashMap::new());

        assert_eq!(settlement.resolved.get("piece"), Some(&UnitCode::H87));
    }

    #[test]
    fn a_recommendation_21_code_alone_resolves() {
        let mut candidates = Candidates::new();
        candidates.offer("aerosol", UnitCode::Xae);

        assert_eq!(candidates.settle(&HashMap::new()).resolved.get("aerosol"), Some(&UnitCode::Xae));
    }

    #[test]
    fn a_contested_notation_resolves_as_its_ruling_says() {
        let mut candidates = Candidates::new();
        candidates.offer("V", UnitCode::Vlt);
        candidates.offer("V", UnitCode::Code2g);
        candidates.offer("rad", UnitCode::C80);
        candidates.offer("rad", UnitCode::C81);
        let rulings = HashMap::from([("V", Ruling::Code(UnitCode::Vlt)), ("rad", Ruling::Ambiguous)]);

        let settlement = candidates.settle(&rulings);

        assert_eq!(settlement.resolved.get("V"), Some(&UnitCode::Vlt));
        assert_eq!(settlement.resolved.get("rad"), None);
        assert_eq!(settlement.contested.keys().copied().collect::<Vec<_>>(), ["V", "rad"]);
    }

    #[test]
    fn a_contested_notation_without_a_ruling_resolves_to_nothing() {
        let mut candidates = Candidates::new();
        candidates.offer("st", UnitCode::G26);
        candidates.offer("st", UnitCode::Sti);

        let settlement = candidates.settle(&HashMap::new());

        assert_eq!(settlement.resolved.get("st"), None);
        assert_eq!(settlement.contested.get("st"), Some(&BTreeSet::from([UnitCode::G26, UnitCode::Sti])));
    }
}
