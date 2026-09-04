//! End-to-end check of the shapefile handoff from the profiler to the ingestor.
//!
//! The fixture is profiled from its path exactly as the pipeline would ([`cassiopeia_data_profiler::profile_path`]),
//! then the resulting profile rides into [`ShapefileIngestor`], which resolves the carrier (bare `.shp`
//! vs zip bundle) and companion files itself: the pipeline never coordinates the sibling set, exactly
//! as KMZ works.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration-test fixtures unwrap on setup failure and panic on an unexpected signal, which is the intended way for the test to abort"
)]

use cassiopeia_common::{channel::ChannelSender, collection::CollectionName, format::DataFormat, signal::Signal};
use cassiopeia_data_profiler::profile_path;
use cassiopeia_ingestor::{ingestor::Ingestor, shapefile::ingestor::ShapefileIngestor};
use cassiopeia_ir::{
    payload::{CollectedPayload, FilePayload, ProfiledPayload},
    record::Record,
};
use dbase::{FieldName, FieldValue, Record as DbaseRecord, TableWriterBuilder};
use shapefile::{Point as ShpPoint, Writer};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::Path,
    sync::mpsc::sync_channel,
    thread,
};
use temp_dir::TempDir;
use zip::{ZipWriter, write::SimpleFileOptions};

/// A plain GEOGCS WGS84 definition, so the fixtures need no reprojection.
const WGS84_WKT: &str =
    r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]]"#;

/// Writes a point layer (`.shp`/`.shx`/`.dbf`/`.prj`) with one string attribute.
fn write_layer(dir: &Path, stem: &str, points: &[(f64, f64, &str)]) {
    let table = TableWriterBuilder::new()
        .add_character_field(FieldName::try_from("name").unwrap(), 50)
        .build_table_info();
    let mut writer = Writer::from_path_with_info(dir.join(format!("{stem}.shp")), table).unwrap();
    for (x, y, name) in points {
        let mut record = DbaseRecord::default();
        record.insert("name".to_string(), FieldValue::Character(Some((*name).to_string())));
        writer.write_shape_and_record(&ShpPoint::new(*x, *y), &record).unwrap();
    }
    drop(writer);
    fs::write(dir.join(format!("{stem}.prj")), WGS84_WKT).unwrap();
}

/// Zips every file in `dir` flat by basename into `archive_path`.
fn zip_dir(dir: &Path, archive_path: &Path) {
    let mut archive = ZipWriter::new(fs::File::create(archive_path).unwrap());
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        archive.start_file(name, SimpleFileOptions::default()).unwrap();
        let mut bytes = Vec::new();
        fs::File::open(&path).unwrap().read_to_end(&mut bytes).unwrap();
        archive.write_all(&bytes).unwrap();
    }
    archive.finish().unwrap();
}

/// Profiles the path exactly as the pipeline would, asserts it is a shapefile, and pairs it with a payload.
fn profiled(path: &Path) -> ProfiledPayload {
    let profile = profile_path(path).unwrap();
    assert_eq!(*profile.format(), DataFormat::Shapefile);
    ProfiledPayload::new(
        CollectedPayload::File(FilePayload::new(path.to_path_buf(), Some(DataFormat::Shapefile))),
        profile,
    )
}

fn ingest(payload: ProfiledPayload) -> Vec<Record> {
    let ingestor = ShapefileIngestor::from_payload(payload, 8).unwrap();
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
fn a_bare_shapefile_profiles_and_ingests_into_one_record_per_feature() {
    let dir = TempDir::new().unwrap();
    write_layer(dir.path(), "stations", &[(14.5, 46.05, "Alpha"), (15.0, 46.10, "Beta")]);

    let records = ingest(profiled(&dir.path().join("stations.shp")));
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| record.collection().is_none()));
    assert!(records.iter().all(|record| record.data().contains_key("geometry")));
}

#[test]
fn a_multi_layer_zip_profiles_as_a_shapefile_and_routes_each_layer_to_its_collection() {
    let layer_dir = TempDir::new().unwrap();
    write_layer(layer_dir.path(), "roads", &[(14.0, 46.0, "R1"), (14.1, 46.1, "R2")]);
    write_layer(layer_dir.path(), "poi", &[(15.0, 47.0, "P1"), (15.1, 47.1, "P2"), (15.2, 47.2, "P3")]);
    let bundle_dir = TempDir::new().unwrap();
    let bundle = bundle_dir.path().join("bundle.zip");
    zip_dir(layer_dir.path(), &bundle);

    let records = ingest(profiled(&bundle));
    let mut per_collection: HashMap<String, usize> = HashMap::new();
    for record in &records {
        let label = record
            .collection()
            .as_ref()
            .map(CollectionName::as_str)
            .expect("a multi-layer bundle tags every record");
        *per_collection.entry(label.to_string()).or_default() += 1;
    }
    assert_eq!(per_collection.get("roads"), Some(&2));
    assert_eq!(per_collection.get("poi"), Some(&3));
}
