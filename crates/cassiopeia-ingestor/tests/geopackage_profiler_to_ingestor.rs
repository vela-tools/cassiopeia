//! End-to-end check of the `GeoPackage` handoff from the profiler to the ingestor.
//!
//! The fixture is written by the `geopackage` crate, profiled from its path exactly as the pipeline
//! would ([`cassiopeia_data_profiler::profile_path`]), then the resulting profile rides into
//! [`GeoPackageIngestor`], which discovers the file's record tables itself.
#![cfg(feature = "geopackage")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration-test fixtures unwrap on setup failure and panic on an unexpected signal, which is the intended way for the test to abort"
)]

use cassiopeia_common::{channel::ChannelSender, collection::CollectionName, format::DataFormat, signal::Signal};
use cassiopeia_data_profiler::profile_path;
use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
use cassiopeia_ingestor::{
    geopackage::{ingestor::GeoPackageIngestor, table_selection::GeoPackageTableSelection},
    ingestor::Ingestor,
};
use cassiopeia_ir::{
    payload::{CollectedPayload, FilePayload, ProfiledPayload},
    record::Record,
};
use geo_types::Point;
use geopackage::{ColumnSpec, GeoPackage, GeometrySpec, NewFeature, TableSchemaBuilder, Value};
use geopackage_core::types::{ColumnType, GeometryType};
use pretty_assertions::assert_eq;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, mpsc::sync_channel},
    thread,
};
use temp_dir::TempDir;

/// Writes an EPSG:4326 point layer named `table` with one `name` text column.
fn write_layer(gpkg: &GeoPackage, table: &str, points: &[(f64, f64, &str)]) {
    gpkg.create_layer(
        &TableSchemaBuilder::new(table)
            .column(ColumnSpec::new("name", ColumnType::Text(None)))
            .geometry(GeometrySpec::new(GeometryType::Point, 4326)),
    )
    .unwrap();
    let features = points
        .iter()
        .map(|(x, y, name)| NewFeature::new(Point::new(*x, *y), vec![Value::Text((*name).to_string())]));
    gpkg.layer(table).unwrap().write_all(features, 0).unwrap();
}

/// Profiles the path exactly as the pipeline would, asserts it is a `GeoPackage`, and pairs it with a
/// payload.
fn profiled(path: &Path) -> ProfiledPayload {
    let profile = profile_path(path, &EncodingHint::NONE).unwrap();
    assert_eq!(*profile.format(), DataFormat::GeoPackage);
    ProfiledPayload::new(
        CollectedPayload::File(FilePayload::local(path.to_path_buf(), Some(DataFormat::GeoPackage), EncodingHint::NONE)),
        profile,
    )
}

fn ingest(payload: ProfiledPayload) -> Vec<Record> {
    let ingestor = GeoPackageIngestor::from_payload(payload, 8, Arc::new(GeoPackageTableSelection::Every)).unwrap();
    let (tx, rx) = sync_channel(8);
    let handle = thread::spawn(move || Box::new(ingestor).ingest(ChannelSender::bounded(tx)));
    let records: Vec<Record> = rx
        .iter()
        .flat_map(|signal| match signal {
            Signal::Data(records) => records,
            Signal::Start | Signal::Stop | Signal::Meta(_) | Signal::Error(_) => panic!("expected a data signal"),
        })
        .collect();
    handle.join().unwrap().unwrap();
    records
}

#[test]
fn a_single_layer_geopackage_profiles_and_ingests_into_one_record_per_feature() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("stations.gpkg");
    let gpkg = GeoPackage::create(&path).unwrap();
    write_layer(&gpkg, "stations", &[(14.5, 46.05, "Alpha"), (15.0, 46.10, "Beta")]);
    gpkg.close().unwrap();

    let records = ingest(profiled(&path));
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| record.collection().is_none()));
    assert!(records.iter().all(|record| record.data().contains_key("geometry")));
}

#[test]
fn a_multi_layer_geopackage_profiles_and_routes_each_layer_to_its_collection() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("network.gpkg");
    let gpkg = GeoPackage::create(&path).unwrap();
    write_layer(&gpkg, "roads", &[(14.0, 46.0, "R1"), (14.1, 46.1, "R2")]);
    write_layer(&gpkg, "poi", &[(15.0, 47.0, "P1"), (15.1, 47.1, "P2"), (15.2, 47.2, "P3")]);
    gpkg.close().unwrap();

    let records = ingest(profiled(&path));
    let mut per_collection: HashMap<String, usize> = HashMap::new();
    for record in &records {
        let label = record
            .collection()
            .as_ref()
            .map(CollectionName::as_str)
            .expect("a multi-layer GeoPackage tags every record");
        *per_collection.entry(label.to_string()).or_default() += 1;
    }
    assert_eq!(per_collection.get("roads"), Some(&2));
    assert_eq!(per_collection.get("poi"), Some(&3));
}
