use cassiopeia_ngsi_ld::entity::value_type::ValueType;
use iri_rs::IriBuf;
use std::collections::HashMap;

/// The datatype IRI each of a set of value types expands to under one `@context`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExpandedValueTypes {
    /// Each value type as written, and the IRI it expands to.
    iris: HashMap<ValueType, IriBuf>,
}

impl ExpandedValueTypes {
    /// Records the IRI one value type expands to.
    pub(crate) fn insert(&mut self, value_type: ValueType, iri: IriBuf) {
        self.iris.insert(value_type, iri);
    }

    /// The IRI `value_type` expands to, when it was among the value types expanded.
    #[must_use]
    pub fn iri(&self, value_type: &ValueType) -> Option<&IriBuf> {
        self.iris.get(value_type)
    }

    /// Whether `first` and `second` name the same datatype: both were expanded, to the same IRI.
    #[must_use]
    pub fn same_datatype(&self, first: &ValueType, second: &ValueType) -> bool {
        match (self.iri(first), self.iri(second)) {
            (Some(first), Some(second)) => first == second,
            (Some(_) | None, None) | (None, Some(_)) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::expanded_value_types::ExpandedValueTypes;
    use cassiopeia_ngsi_ld::entity::value_type::ValueType;
    use iri_rs::IriBuf;

    fn value_type(text: &str) -> ValueType {
        ValueType::new(text).unwrap()
    }

    fn expanded(pairs: &[(&str, &str)]) -> ExpandedValueTypes {
        let mut expanded = ExpandedValueTypes::default();
        for (text, iri) in pairs {
            expanded.insert(value_type(text), IriBuf::new((*iri).to_owned()).unwrap());
        }
        expanded
    }

    #[test]
    fn two_spellings_expanding_to_one_iri_name_the_same_datatype() {
        let expanded = expanded(&[
            ("DateTime", "https://uri.etsi.org/ngsi-ld/DateTime"),
            ("ngsi-ld:DateTime", "https://uri.etsi.org/ngsi-ld/DateTime"),
            ("Date", "https://uri.etsi.org/ngsi-ld/Date"),
        ]);

        assert!(expanded.same_datatype(&value_type("DateTime"), &value_type("ngsi-ld:DateTime")));
        assert!(!expanded.same_datatype(&value_type("DateTime"), &value_type("Date")));
    }

    #[test]
    fn a_value_type_that_was_not_expanded_names_no_datatype() {
        let expanded = expanded(&[("DateTime", "https://uri.etsi.org/ngsi-ld/DateTime")]);

        assert!(!expanded.same_datatype(&value_type("DateTime"), &value_type("Time")));
        assert!(!expanded.same_datatype(&value_type("Time"), &value_type("Time")));
    }
}
