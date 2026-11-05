use crate::conversion::value_reading::ValueReading;

/// How a `ListProperty` builds its `valueList` (ETSI GS CIM 009 v1.9.1 clause 4.5.21.2): every
/// source part gathered into one list, its items kept as read or each read one way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListConversion {
    /// Every item kept as the source holds it: no transformation, or `array`.
    AsRead,
    /// Every item read as the reading names, so the transformation types the list's content.
    Each(ValueReading),
}
