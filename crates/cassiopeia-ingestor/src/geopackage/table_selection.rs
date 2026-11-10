use cassiopeia_common::collection::CollectionName;

/// Which of a `GeoPackage`'s record tables the ingestor reads.
///
/// The choice follows the input's mapping binding: a lone `mapping` maps every record whatever its
/// collection, so every table is read, while a `mappings` list maps only the collections it names, so
/// a table it does not name is left unread and cannot affect the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeoPackageTableSelection {
    /// Every record table is read.
    Every,
    /// Only the record tables with one of these names are read.
    Listed(Vec<CollectionName>),
}

impl GeoPackageTableSelection {
    /// Whether the record table `gpkg_contents` lists as `table` is read.
    ///
    /// A table is named exactly as its collection label is matched: verbatim, case included.
    #[must_use]
    pub fn selects(&self, table: &CollectionName) -> bool {
        match self {
            GeoPackageTableSelection::Every => true,
            GeoPackageTableSelection::Listed(tables) => tables.contains(table),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::geopackage::table_selection::GeoPackageTableSelection;
    use cassiopeia_common::collection::CollectionName;

    #[test]
    fn every_table_is_read_when_one_mapping_maps_every_record() {
        assert!(GeoPackageTableSelection::Every.selects(&CollectionName::from("v_names")));
    }

    #[test]
    fn a_listed_selection_reads_only_the_tables_it_names_verbatim() {
        let selection = GeoPackageTableSelection::Listed(vec![CollectionName::from("stations"), CollectionName::from("operators")]);

        assert!(selection.selects(&CollectionName::from("operators")));
        assert!(!selection.selects(&CollectionName::from("v_names")));
        assert!(!selection.selects(&CollectionName::from("Stations")));
    }
}
