use crate::timestamp_loss::TimestampLoss;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use indexmap::Equivalent;

/// Which timestamp of which attribute was lost: the key the attribute timestamp sink files its
/// records under.
///
/// The key pairs the loss with the attribute, so one attribute losing two different timestamps on
/// the same records (its own value and its `expiresAt`) is two records, each reported under the code
/// that says what its loss cost.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AttributeLoss {
    /// Which of the attribute's timestamps was lost.
    pub loss: TimestampLoss,
    /// The attribute that lost it.
    pub attribute: NameBuf,
}

/// An [`AttributeLoss`] that borrows its attribute, so a sink can find a record without owning the
/// attribute name.
///
/// Its derived `Hash` feeds the same fields in the same order as [`AttributeLoss`]'s, a reference
/// hashing as the value it points at, so the two agree on every key, as [`Equivalent`] requires.
#[derive(Clone, Copy, Debug, Hash)]
pub(crate) struct AttributeLossRef<'a> {
    /// Which of the attribute's timestamps was lost.
    pub(crate) loss: TimestampLoss,
    /// The attribute that lost it.
    pub(crate) attribute: &'a NameBuf,
}

impl AttributeLossRef<'_> {
    /// The owned key a new record is filed under.
    pub(crate) fn to_owned_key(self) -> AttributeLoss {
        AttributeLoss {
            loss: self.loss,
            // The sink outlives the borrowed attribute name, so the key must own it; only the first
            // failure of an attribute pays for this.
            attribute: self.attribute.clone(),
        }
    }
}

// `Equivalent` has no derive: it relates two distinct types, which is what lets the sink look a
// record up by a borrowed key.
impl Equivalent<AttributeLoss> for AttributeLossRef<'_> {
    fn equivalent(&self, key: &AttributeLoss) -> bool {
        self.loss == key.loss && *self.attribute == key.attribute
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_loss::{AttributeLoss, AttributeLossRef},
        timestamp_loss::TimestampLoss,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use indexmap::Equivalent;
    use pretty_assertions::assert_eq;
    use std::hash::{BuildHasher, RandomState};

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn a_borrowed_key_is_equivalent_to_and_hashes_like_its_owned_key() {
        let attribute = name("temperature");
        let borrowed = AttributeLossRef {
            loss: TimestampLoss::ExpiresAt,
            attribute: &attribute,
        };
        let owned = borrowed.to_owned_key();
        let hasher = RandomState::new();

        assert!(borrowed.equivalent(&owned));
        assert_eq!(hasher.hash_one(borrowed), hasher.hash_one(&owned));
    }

    #[test]
    fn a_borrowed_key_is_not_equivalent_to_another_loss_or_another_attribute() {
        let attribute = name("temperature");
        let borrowed = AttributeLossRef {
            loss: TimestampLoss::ExpiresAt,
            attribute: &attribute,
        };

        assert!(!borrowed.equivalent(&AttributeLoss {
            loss: TimestampLoss::ObservedAt,
            attribute: name("temperature"),
        }));
        assert!(!borrowed.equivalent(&AttributeLoss {
            loss: TimestampLoss::ExpiresAt,
            attribute: name("humidity"),
        }));
    }
}
