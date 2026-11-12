use cassiopeia_ngsi_ld::entity::name::NameBuf;
use indexmap::Equivalent;

/// An attribute that lost its `unitCode` to text that is no UN/CEFACT Recommendation No. 20 code:
/// the key the unknown-unit-code sink files its records under.
///
/// The text is part of the key, unlike the example text of an unreadable timestamp. A `unitCode`
/// usually comes from the mapping, or from a column that holds a handful of units, so the texts one
/// attribute loses stay few however many records carry them, and each is a different thing to fix:
/// `ug/m3` wants the `cefact_unit` filter, a typo such as `GQQ` wants correcting.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct UnknownUnitCode {
    /// The top-level attribute that was emitted without its `unitCode`.
    pub attribute: NameBuf,
    /// The text given as the `unitCode`, as the run reports it: text without the whitespace around
    /// it, and any other value as its JSON rendering.
    pub text: Box<str>,
}

/// An [`UnknownUnitCode`] that borrows its parts, so a sink can count a repeat without owning them.
///
/// Its derived `Hash` feeds the same fields in the same order as [`UnknownUnitCode`]'s, a reference
/// hashing as the value it points at and a `Box<str>` as the `str` it holds, so the two agree on every
/// key, as [`Equivalent`] requires.
#[derive(Clone, Copy, Debug, Hash)]
pub(crate) struct UnknownUnitCodeRef<'a> {
    /// The top-level attribute that was emitted without its `unitCode`.
    pub(crate) attribute: &'a NameBuf,
    /// The text given as the `unitCode`.
    pub(crate) text: &'a str,
}

impl UnknownUnitCodeRef<'_> {
    /// The owned key a new record is filed under.
    pub(crate) fn to_owned_key(self) -> UnknownUnitCode {
        UnknownUnitCode {
            // The sink outlives the borrowed attribute name, so the key must own it; only the first
            // record of an attribute and text pays for this.
            attribute: self.attribute.clone(),
            text: Box::from(self.text),
        }
    }
}

// `Equivalent` has no derive: it relates two distinct types, which is what lets the sink look a
// record up by a borrowed key.
impl Equivalent<UnknownUnitCode> for UnknownUnitCodeRef<'_> {
    fn equivalent(&self, key: &UnknownUnitCode) -> bool {
        *self.attribute == key.attribute && self.text == key.text.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use crate::unknown_unit_code::{UnknownUnitCode, UnknownUnitCodeRef};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use indexmap::Equivalent;
    use pretty_assertions::assert_eq;
    use std::hash::{BuildHasher, RandomState};

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn a_borrowed_key_is_equivalent_to_and_hashes_like_its_owned_key() {
        let attribute = name("pm10");
        let borrowed = UnknownUnitCodeRef {
            attribute: &attribute,
            text: "ug/m3",
        };
        let owned = borrowed.to_owned_key();
        let hasher = RandomState::new();

        assert!(borrowed.equivalent(&owned));
        assert_eq!(hasher.hash_one(borrowed), hasher.hash_one(&owned));
    }

    #[test]
    fn a_borrowed_key_is_not_equivalent_to_another_attribute_or_another_text() {
        let attribute = name("pm10");
        let borrowed = UnknownUnitCodeRef {
            attribute: &attribute,
            text: "ug/m3",
        };

        assert!(!borrowed.equivalent(&UnknownUnitCode {
            attribute: name("no2"),
            text: Box::from("ug/m3"),
        }));
        assert!(!borrowed.equivalent(&UnknownUnitCode {
            attribute: name("pm10"),
            text: Box::from("GQQ"),
        }));
    }
}
