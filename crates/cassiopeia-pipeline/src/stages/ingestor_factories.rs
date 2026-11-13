use crate::{
    error::PipelineError,
    stages::{
        misshapen_rows_report::report_misshapen_rows,
        nonexistent_dates_report::report_nonexistent_dates,
        renamed_columns_report::report_renamed_columns,
        renamed_readings_report::report_renamed_readings,
        snapshot_read_rule::check_snapshot_read,
        unreadable_locations_report::report_unreadable_locations,
        unreadable_phenomenon_times_report::report_unreadable_phenomenon_times,
        unreadable_tables_report::report_unreadable_tables,
    },
};
use cassiopeia_common::{collection::CollectionName, format::DataFormat, input::Input};
use cassiopeia_ingestor::{
    csv::ingestor::CsvIngestor,
    geojson::ingestor::GeoJsonIngestor,
    geopackage::{ingestor::GeoPackageIngestor, table_selection::GeoPackageTableSelection},
    grib::ingestor::GribIngestor,
    ingestor::Ingestor,
    json::{ingestor::JsonIngestor, record_selection::JsonRecordSelection},
    kml::ingestor::KmlIngestor,
    netcdf::ingestor::NetCdfIngestor,
    sensorthings::{ingest_losses::SensorThingsLosses, ingestor::SensorThingsIngestor},
    shapefile::ingestor::ShapefileIngestor,
    xml::ingestor::XmlIngestor,
};
use cassiopeia_ir::payload::ProfiledPayload;
use cassiopeia_manifest::{
    mapping_binding::{CollectionMapping, MappingBinding},
    output::temporal::TemporalRepresentation,
};
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::sync::Arc;

/// Builds a boxed ingestor for one profiled payload, reporting to the sink what building it found
/// about the source. One factory exists per concrete format, which is why `DataFormat::Auto`
/// (resolved away by the profiler before this point) has no entry and cannot reach an ingestor. A
/// factory may carry what its format needs to know about the lane's input, such as how JSON records
/// are selected, or about the run's output, such as whether a SensorThings snapshot's records are
/// kept.
pub(crate) type IngestorFactory = Box<dyn Fn(ProfiledPayload, usize, &dyn DiagnosticSink) -> Result<PreparedIngestor, PipelineError> + Send>;

/// Reports to the sink what an ingestor met while it read its source, once it has read all of it,
/// and returns how many warnings that was.
pub(crate) type AfterIngest = Box<dyn FnOnce(&dyn DiagnosticSink) -> u64>;

/// An ingestor ready to run, how many warnings building it reported, and what is left to report
/// once it has run.
pub(crate) struct PreparedIngestor {
    /// The ingestor.
    pub(crate) ingestor: Box<dyn Ingestor>,
    /// How many warnings building it reported.
    pub(crate) warnings: u64,
    /// What the ingestor gathers while it runs, reported once it has run; `None` for an ingestor
    /// that gathers nothing.
    pub(crate) after_ingest: Option<AfterIngest>,
}

impl PreparedIngestor {
    /// An ingestor whose building reported nothing and that gathers nothing while it runs.
    fn quiet(ingestor: Box<dyn Ingestor>) -> PreparedIngestor {
        PreparedIngestor {
            ingestor,
            warnings: 0,
            after_ingest: None,
        }
    }
}

/// The ingestor factory of each supported format, paired with the format.
///
/// `mapping_binding` is the lane's binding, which decides how a JSON source is split into records and
/// which tables of a `GeoPackage` are read; `temporal` is the run's temporal output, which decides
/// which SensorThings snapshots are read. A format whose ingestor gathers what it meets while it
/// reads, such as a CSV file's misshapen rows or a `NetCDF` file's nonexistent dates, reports that once
/// the ingestor has run.
pub(crate) fn ingestor_factories(mapping_binding: &MappingBinding, temporal: Option<TemporalRepresentation>) -> [(DataFormat, IngestorFactory); 11] {
    let json_selection = Arc::new(json_record_selection(mapping_binding));
    let geopackage_selection = Arc::new(geopackage_table_selection(mapping_binding));
    [
        (
            DataFormat::Csv,
            Box::new(|payload, batch, sink| {
                let ingestor = CsvIngestor::from_payload(payload, batch)?;
                let warnings = report_renamed_columns(sink, ingestor.source(), ingestor.renamed_columns());
                let misshapen = ingestor.misshapen_rows();
                // The ingestor is given up when it runs, while the report names its source after.
                let source = ingestor.source().clone();
                Ok(PreparedIngestor {
                    ingestor: Box::new(ingestor),
                    warnings,
                    after_ingest: Some(Box::new(move |sink: &dyn DiagnosticSink| {
                        report_misshapen_rows(sink, &source, &misshapen.lock())
                    })),
                })
            }),
        ),
        (
            DataFormat::Json,
            Box::new(move |payload, batch, _| {
                Ok(PreparedIngestor::quiet(Box::new(JsonIngestor::from_payload(
                    payload,
                    batch,
                    Arc::clone(&json_selection),
                )?)))
            }),
        ),
        (
            DataFormat::GeoJson,
            Box::new(|payload, batch, _| Ok(PreparedIngestor::quiet(Box::new(GeoJsonIngestor::from_payload(payload, batch)?)))),
        ),
        (
            DataFormat::Kml,
            Box::new(|payload, batch, _| Ok(PreparedIngestor::quiet(Box::new(KmlIngestor::from_payload(payload, batch)?)))),
        ),
        (
            DataFormat::Kmz,
            Box::new(|payload, batch, _| Ok(PreparedIngestor::quiet(Box::new(KmlIngestor::from_payload(payload, batch)?)))),
        ),
        (
            DataFormat::Grib,
            Box::new(|payload, batch, _| Ok(PreparedIngestor::quiet(Box::new(GribIngestor::from_payload(payload, batch)?)))),
        ),
        (
            DataFormat::Shapefile,
            Box::new(|payload, batch, _| Ok(PreparedIngestor::quiet(Box::new(ShapefileIngestor::from_payload(payload, batch)?)))),
        ),
        (
            DataFormat::GeoPackage,
            Box::new(move |payload, batch, _| {
                let ingestor = GeoPackageIngestor::from_payload(payload, batch, Arc::clone(&geopackage_selection))?;
                let unreadable = ingestor.unreadable_tables();
                // The ingestor is given up when it runs, while the report names its source after.
                let source = ingestor.source().clone();
                Ok(PreparedIngestor {
                    ingestor: Box::new(ingestor),
                    warnings: 0,
                    after_ingest: Some(Box::new(move |sink: &dyn DiagnosticSink| {
                        report_unreadable_tables(sink, &source, &unreadable.lock())
                    })),
                })
            }),
        ),
        (
            DataFormat::NetCdf,
            Box::new(|payload, batch, _| {
                let ingestor = NetCdfIngestor::from_payload(payload, batch)?;
                let nonexistent = ingestor.nonexistent_dates();
                // The ingestor is given up when it runs, while the report names its source after.
                let source = ingestor.source().clone();
                Ok(PreparedIngestor {
                    ingestor: Box::new(ingestor),
                    warnings: 0,
                    after_ingest: Some(Box::new(move |sink: &dyn DiagnosticSink| {
                        report_nonexistent_dates(sink, &source, nonexistent.lock().variables())
                    })),
                })
            }),
        ),
        (
            DataFormat::Xml,
            Box::new(|payload, batch, _| Ok(PreparedIngestor::quiet(Box::new(XmlIngestor::from_payload(payload, batch)?)))),
        ),
        (
            DataFormat::SensorThings,
            Box::new(move |payload, batch, _| prepare_sensorthings(payload, batch, temporal)),
        ),
    ]
}

/// Builds the ingestor of a SensorThings snapshot, refusing one whose records the run's `temporal`
/// output would not keep.
///
/// Building the ingestor reads the snapshot's header, so the read it states is known here, before
/// any record is read.
fn prepare_sensorthings(payload: ProfiledPayload, batch: usize, temporal: Option<TemporalRepresentation>) -> Result<PreparedIngestor, PipelineError> {
    let ingestor = SensorThingsIngestor::from_payload(payload, batch)?;
    check_snapshot_read(ingestor.read(), temporal, ingestor.source())?;
    let losses = ingestor.losses();
    // The ingestor is given up when it runs, while the reports name its source after.
    let source = ingestor.source().clone();
    Ok(PreparedIngestor {
        ingestor: Box::new(ingestor),
        warnings: 0,
        after_ingest: Some(Box::new(move |sink: &dyn DiagnosticSink| {
            report_sensorthings_losses(sink, &source, &losses.lock())
        })),
    })
}

/// Reports what reading a SensorThings snapshot lost or changed, one warning per kind of loss, and
/// returns how many losses that was.
fn report_sensorthings_losses(sink: &dyn DiagnosticSink, source: &Input, losses: &SensorThingsLosses) -> u64 {
    [
        report_renamed_readings(sink, source, losses.renamed_readings()),
        report_unreadable_locations(sink, source, losses.unreadable_locations()),
        report_unreadable_phenomenon_times(sink, source, losses.unreadable_phenomenon_times()),
    ]
    .into_iter()
    .fold(0, u64::saturating_add)
}

/// Picks how a lane's JSON source is split into records from the lane's mapping binding.
///
/// A lone `mapping` keeps the lone-array convention; a `mappings` list reads the nested collections
/// it names.
pub(crate) fn json_record_selection(mapping_binding: &MappingBinding) -> JsonRecordSelection {
    match mapping_binding {
        MappingBinding::Single { .. } => JsonRecordSelection::LoneArray,
        MappingBinding::Collections { mappings } => JsonRecordSelection::Collections(listed_collections(mappings)),
    }
}

/// Picks which tables of a lane's `GeoPackage` are read from the lane's mapping binding.
///
/// A lone `mapping` maps every record, so every table is read; a `mappings` list maps only the
/// collections it names, so only the tables of those names are.
pub(crate) fn geopackage_table_selection(mapping_binding: &MappingBinding) -> GeoPackageTableSelection {
    match mapping_binding {
        MappingBinding::Single { .. } => GeoPackageTableSelection::Every,
        MappingBinding::Collections { mappings } => GeoPackageTableSelection::Listed(listed_collections(mappings)),
    }
}

/// The collection labels a `mappings` list names, in order.
fn listed_collections(mappings: &[CollectionMapping]) -> Vec<CollectionName> {
    // The labels outlive the binding, which the lane keeps for routing.
    mappings.iter().map(|entry| entry.collection().clone()).collect()
}

#[cfg(test)]
mod tests {
    use crate::stages::ingestor_factories::{geopackage_table_selection, json_record_selection};
    use cassiopeia_common::collection::CollectionName;
    use cassiopeia_ingestor::{geopackage::table_selection::GeoPackageTableSelection, json::record_selection::JsonRecordSelection};
    use cassiopeia_manifest::mapping_binding::{CollectionMapping, MappingBinding};
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    #[test]
    fn a_single_mapping_keeps_the_lone_array_convention_for_json() {
        let binding = MappingBinding::Single {
            mapping: PathBuf::from("m.json5"),
        };

        assert_eq!(json_record_selection(&binding), JsonRecordSelection::LoneArray);
    }

    #[test]
    fn a_mappings_list_selects_its_collections_for_json_in_order() {
        let binding = MappingBinding::Collections {
            mappings: vec![
                CollectionMapping::new(CollectionName::from("data.lines"), PathBuf::from("line.json5")),
                CollectionMapping::new(CollectionName::from("data.lines.stops"), PathBuf::from("stop.json5")),
            ],
        };

        assert_eq!(
            json_record_selection(&binding),
            JsonRecordSelection::Collections(vec![CollectionName::from("data.lines"), CollectionName::from("data.lines.stops")])
        );
    }

    #[test]
    fn a_single_mapping_reads_every_geopackage_table() {
        let binding = MappingBinding::Single {
            mapping: PathBuf::from("m.json5"),
        };

        assert_eq!(geopackage_table_selection(&binding), GeoPackageTableSelection::Every);
    }

    #[test]
    fn a_mappings_list_reads_only_the_geopackage_tables_it_names() {
        let binding = MappingBinding::Collections {
            mappings: vec![
                CollectionMapping::new(CollectionName::from("vg5000_gem"), PathBuf::from("municipality.json5")),
                CollectionMapping::new(CollectionName::from("vg5000_krs"), PathBuf::from("district.json5")),
            ],
        };

        assert_eq!(
            geopackage_table_selection(&binding),
            GeoPackageTableSelection::Listed(vec![CollectionName::from("vg5000_gem"), CollectionName::from("vg5000_krs")])
        );
    }
}
