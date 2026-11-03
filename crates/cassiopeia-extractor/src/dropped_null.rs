use cassiopeia_ngsi_ld::entity::name::NameBuf;
use derive_more::Display;

/// Where in a source value a null that was left out sat.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
pub enum NullSite {
    /// An item of an array.
    #[display("array items")]
    ArrayItem,
    /// The value of an object member.
    #[display("object members")]
    ObjectMember,
}

/// One attribute whose source value held nulls that were left out of it, and where they sat.
///
/// `null` cannot be used as a value anywhere in NGSI-LD (ETSI GS CIM 009 v1.9.1 clause 4.5.0): a
/// broker running JSON-LD expansion would remove every null member and list item without a word
/// (clause 5.2.1). Leaving them out at extraction keeps the attribute, and its other members and
/// items, while naming the loss. The attribute and the site together are the diagnostic and its
/// identity, so a run reports one line per attribute and site, however many records lost a null
/// there.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
#[display("Attribute `{attribute}` was written without its null {site}, which NGSI-LD does not allow")]
pub struct DroppedNull {
    /// The attribute whose value lost the nulls.
    pub attribute: NameBuf,
    /// Where in the value they sat.
    pub site: NullSite,
}

#[cfg(test)]
mod tests {
    use crate::dropped_null::{DroppedNull, NullSite};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::{assert_eq, assert_ne};

    fn dropped(attribute: &str, site: NullSite) -> DroppedNull {
        DroppedNull {
            attribute: NameBuf::new(attribute).expect("valid name"),
            site,
        }
    }

    #[test]
    fn the_message_names_the_attribute_and_the_site() {
        let message = dropped("tags", NullSite::ArrayItem).to_string();

        assert!(message.contains("`tags`"), "{message}");
        assert!(message.contains("array items"), "{message}");
    }

    #[test]
    fn the_same_attribute_and_site_are_one_diagnostic_and_another_site_is_another() {
        assert_eq!(dropped("reading", NullSite::ObjectMember), dropped("reading", NullSite::ObjectMember));
        assert_ne!(dropped("reading", NullSite::ObjectMember), dropped("reading", NullSite::ArrayItem));
    }
}
