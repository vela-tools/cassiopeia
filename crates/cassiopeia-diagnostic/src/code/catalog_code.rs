use strum::{Display, EnumCount, EnumIter};

/// Why a Smart Data Models catalog operation did not fully succeed.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum CatalogCode {
    /// One catalog schema could not be downloaded.
    SchemaDownloadFailed,
    /// A catalog download finished with some schemas missing.
    SchemaDownloadIncomplete,
}

#[cfg(test)]
mod tests {
    use crate::code::catalog_code::CatalogCode;

    #[test]
    fn a_catalog_code_renders_a_kebab_case_token() {
        assert_eq!(CatalogCode::SchemaDownloadFailed.to_string(), "schema-download-failed");
    }
}
