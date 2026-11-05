/// How a `JsonProperty` builds its `json` member, "a raw JSON object (or array of objects)" (ETSI GS
/// CIM 009 v1.9.1 clause 4.5.24.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JsonConversion {
    /// The value as the source holds it, or as nested `mappings` assemble it: what a `JsonProperty`
    /// naming no transformation takes.
    Verbatim,
    /// Text read as the JSON it is written as, the `json` transformation.
    Json,
    /// A JSON object, or one read from its JSON text, the `object` transformation.
    Object,
    /// Every source part gathered into one JSON array, the `array` transformation.
    Array,
}
