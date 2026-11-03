use derive_more::{AsRef, Display, From};

/// The key a CSV column's values are read by in a record, as a mapping addresses it with
/// `this['key']`.
///
/// Any text can be a key: a header is kept as written, so its Unicode and punctuation stay
/// addressable from a mapping.
#[derive(AsRef, Clone, Debug, Display, Eq, From, Hash, PartialEq)]
#[as_ref(str)]
pub struct ColumnKey(String);

#[cfg(test)]
mod tests {
    use crate::column_key::ColumnKey;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_column_key_reads_and_renders_as_its_text() {
        let key = ColumnKey::from("NO2 HMW µg/m³".to_owned());
        let text: &str = key.as_ref();

        assert_eq!(text, "NO2 HMW µg/m³");
        assert_eq!(key.to_string(), "NO2 HMW µg/m³");
    }
}
