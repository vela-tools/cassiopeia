/// The JSON nulls one reading of JSON into a [`Value`](crate::value::types::Value) left out, counted
/// by where they sat.
///
/// A null cannot sit anywhere inside an NGSI-LD Value (ETSI GS CIM 009 v1.9.1 clauses 3.1 and
/// 4.5.0), so reading JSON leaves each one out and counts it here, letting the stage that read the
/// source report what the value lost.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OmittedNulls {
    /// Array items that were null.
    pub array_items: u64,
    /// Object members whose value was null.
    pub object_members: u64,
}

impl OmittedNulls {
    /// Whether nothing was left out.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.array_items == 0 && self.object_members == 0
    }
}

#[cfg(test)]
mod tests {
    use crate::value::omitted_nulls::OmittedNulls;

    #[test]
    fn a_fresh_count_is_empty() {
        assert!(OmittedNulls::default().is_empty());
    }

    #[test]
    fn a_count_holding_an_omitted_item_or_member_is_not_empty() {
        assert!(
            !OmittedNulls {
                array_items: 1,
                object_members: 0,
            }
            .is_empty()
        );
        assert!(
            !OmittedNulls {
                array_items: 0,
                object_members: 1,
            }
            .is_empty()
        );
    }
}
