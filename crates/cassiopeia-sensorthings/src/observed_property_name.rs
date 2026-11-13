use derive_more::{Display, From};

/// The `name` of an `ObservedProperty` (OGC 18-088 section 8.2.6), which a reading is named after.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, From)]
pub struct ObservedPropertyName(Box<str>);

#[cfg(test)]
mod tests {
    use crate::observed_property_name::ObservedPropertyName;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_name_is_displayed_as_served_and_ordered_by_its_text() {
        let pm10 = ObservedPropertyName::from(Box::<str>::from("pm10"));
        let no2 = ObservedPropertyName::from(Box::<str>::from("no2"));

        assert_eq!(pm10.to_string(), "pm10");
        assert!(no2 < pm10);
    }
}
