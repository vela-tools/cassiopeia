use derive_more::Display;

/// A key in the wizard's editing tree.
///
/// The tree carries two kinds of key: a schema-derived or user-entered attribute name, and a
/// synthetic choice of a `oneOf` group. The typed key preserves the exact display text used by the
/// editor while keeping the two cases distinct.
///
/// The name a `SchemaField` carries is deliberately pre-validation text: the wizard lets the user
/// type any name and only validates it into a [`NameBuf`](cassiopeia_ngsi_ld::entity::name::NameBuf)
/// when the mapping is saved.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
pub enum EditorKey {
    /// A schema-derived or user-entered attribute name.
    #[display("{_0}")]
    SchemaField(String),

    /// One synthetic choice of a `oneOf` group, rendered as `Option <index> - <title>`.
    #[display("Option {index} - {title}")]
    OneOfOption {
        /// The one-based position of this choice in its `oneOf` group.
        index: u32,
        /// The choice's title, taken from the schema or a `Choice <n>` fallback.
        title: String,
    },
}

impl EditorKey {
    /// Whether this key is a synthetic `oneOf` choice.
    #[must_use]
    pub const fn is_one_of_option(&self) -> bool {
        matches!(self, EditorKey::OneOfOption { .. })
    }

    /// The attribute name a `SchemaField` carries, or `None` for a `oneOf` choice.
    #[must_use]
    pub fn schema_field_name(&self) -> Option<&str> {
        match self {
            EditorKey::SchemaField(name) => Some(name),
            EditorKey::OneOfOption { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::screen::wizard::editor_key::EditorKey;

    #[test]
    fn a_schema_field_displays_as_its_name() {
        let key = EditorKey::SchemaField("temperature".to_string());
        assert_eq!(key.to_string(), "temperature");
        assert!(!key.is_one_of_option());
        assert_eq!(key.schema_field_name(), Some("temperature"));
    }

    #[test]
    fn a_oneof_option_reproduces_the_legacy_display_string() {
        let key = EditorKey::OneOfOption {
            index: 1,
            title: "GeoJSON Point".to_string(),
        };
        assert_eq!(key.to_string(), "Option 1 - GeoJSON Point");
        assert!(key.is_one_of_option());
        assert_eq!(key.schema_field_name(), None);
    }
}
