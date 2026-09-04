use strum::{Display, EnumCount, EnumIter};

/// Why an entity's JSON Schema check did not pass cleanly.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum SchemaCode {
    /// The entity did not conform to the schema its type is checked against.
    Nonconformant,
    /// The entity's type has no schema, so it was not checked.
    Absent,
    /// A schema exists but could not be read, parsed, or compiled.
    Unusable,
    /// A remote validation schema could not be fetched at run setup.
    FetchFailed,
}

#[cfg(test)]
mod tests {
    use crate::code::schema_code::SchemaCode;

    #[test]
    fn a_schema_code_renders_a_kebab_case_token() {
        assert_eq!(SchemaCode::Nonconformant.to_string(), "nonconformant");
    }
}
