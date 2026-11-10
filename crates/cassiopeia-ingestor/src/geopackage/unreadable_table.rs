use crate::geopackage::error::GeoPackageIngestError;
use cassiopeia_common::collection::CollectionName;
use std::sync::Arc;

/// A record table the ingest skipped because it failed before its first row was read.
///
/// The reason is shared, so the run's report of the table and the error of a file none of whose
/// tables could be read can both hold it.
#[derive(Debug, Clone, thiserror::Error)]
#[error("the GeoPackage table '{table}' cannot be read")]
pub struct UnreadableTable {
    /// The table's name, as `gpkg_contents` lists it.
    pub table: CollectionName,
    /// Why it cannot be read.
    #[source]
    pub reason: Arc<GeoPackageIngestError>,
}

#[cfg(test)]
mod tests {
    use crate::geopackage::{error::GeoPackageIngestError, unreadable_table::UnreadableTable};
    use cassiopeia_common::collection::CollectionName;
    use pretty_assertions::assert_eq;
    use std::{error::Error, sync::Arc};

    #[test]
    fn an_unreadable_table_names_itself_and_carries_its_reason_as_its_source() {
        let unreadable = UnreadableTable {
            table: CollectionName::from("v_names"),
            reason: Arc::new(GeoPackageIngestError::NoGeometryColumn { table: "v_names".to_string() }),
        };

        assert_eq!(unreadable.to_string(), "the GeoPackage table 'v_names' cannot be read");
        assert_eq!(
            unreadable.source().map(ToString::to_string).as_deref(),
            Some("the GeoPackage table 'v_names' is listed as features, but gpkg_geometry_columns names no geometry column for it")
        );
    }
}
