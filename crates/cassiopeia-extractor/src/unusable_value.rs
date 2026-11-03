use crate::{attribute_path::AttributePath, required_value::RequiredValue};

/// One attribute, sub-attribute, or instance dropped because its value is not what its NGSI-LD kind
/// has to hold: the key the unusable-value sink files its records under.
///
/// The path and the requirement together are the failure; the values that broke it differ from
/// record to record, so the sink keeps the first as an example rather than keying on it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct UnusableValue {
    /// Where the declaration that lost its value sits in the mapping.
    pub attribute: AttributePath,
    /// What its kind needed the value to be.
    pub required: RequiredValue,
}
