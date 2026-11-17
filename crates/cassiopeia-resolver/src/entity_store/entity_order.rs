use cassiopeia_ir::source_position::SourcePosition;
use rayon::prelude::*;
use urn_rs::Urn;

/// Orders entity ids by the position of each one's first record, the order a store hands its
/// entities on in.
///
/// Every fragment of a run stands at a position of its own, so two ids share a first position only
/// when a caller wrote two fragments at one; those follow one another in the bytewise order of their
/// ids, so the order is the same in every run all the same.
pub(crate) fn in_source_order(firsts: Vec<(SourcePosition, Urn)>) -> Vec<Urn> {
    // The sort moves small keys, each an id's first position and where the id sits, rather than
    // every id through every pass of the sort; each id then moves once, into its place.
    let (mut keys, ids): (Vec<(SourcePosition, usize)>, Vec<Urn>) = firsts.into_iter().enumerate().map(|(index, (first, id))| ((first, index), id)).unzip();
    // Each id appears once, so an unstable sort gives one order; sorting in parallel keeps the one
    // pass over every id of the run from showing in the run's time.
    keys.par_sort_unstable_by(|(left_first, left), (right_first, right)| {
        left_first.cmp(right_first).then_with(|| ids[*left].as_str().cmp(ids[*right].as_str()))
    });
    let mut ids: Vec<Option<Urn>> = ids.into_iter().map(Some).collect();
    keys.into_iter().filter_map(|(_, index)| ids.get_mut(index).and_then(Option::take)).collect()
}

#[cfg(test)]
mod tests {
    use crate::entity_store::entity_order::in_source_order;
    use cassiopeia_ir::source_position::SourcePosition;
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    #[test]
    fn ids_follow_the_positions_of_their_first_records() {
        let first = SourcePosition::first_of(0);
        let ordered = in_source_order(vec![
            (first.advanced(7), urn("urn:ngsi-ld:Station:A")),
            (first.advanced(2), urn("urn:ngsi-ld:Station:C")),
            (SourcePosition::first_of(1), urn("urn:ngsi-ld:Station:B")),
            (first, urn("urn:ngsi-ld:Station:D")),
        ]);
        assert_eq!(
            ordered,
            [
                urn("urn:ngsi-ld:Station:D"),
                urn("urn:ngsi-ld:Station:C"),
                urn("urn:ngsi-ld:Station:A"),
                urn("urn:ngsi-ld:Station:B"),
            ]
        );
    }

    #[test]
    fn ids_whose_first_record_is_the_same_follow_the_order_of_their_text() {
        let shared = SourcePosition::first_of(0).advanced(3);
        let ordered = in_source_order(vec![
            (shared, urn("urn:ngsi-ld:Station:S1")),
            (shared, urn("urn:ngsi-ld:Operator:O1")),
            (SourcePosition::first_of(0), urn("urn:ngsi-ld:Station:S0")),
        ]);
        assert_eq!(
            ordered,
            [urn("urn:ngsi-ld:Station:S0"), urn("urn:ngsi-ld:Operator:O1"), urn("urn:ngsi-ld:Station:S1")]
        );
    }

    #[test]
    fn no_ids_order_into_none() {
        assert_eq!(in_source_order(Vec::new()), Vec::<Urn>::new());
    }
}
