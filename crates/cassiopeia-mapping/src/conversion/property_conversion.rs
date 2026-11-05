use crate::conversion::value_reading::ValueReading;

/// How a `Property` builds its value: one value read one way, or every value gathered into a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyConversion {
    /// One value, read as the reading names.
    One(ValueReading),
    /// Every source part gathered into one JSON array, the `array` transformation.
    Array,
}
