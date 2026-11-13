//! Heap usage of the declared-format path, measured by a counting global allocator.
//!
//! A declared format reaches its inspector through a read-only mapping of the file, so the file is
//! never copied onto the heap. The allocator counts heap blocks only, and mapped pages are not heap
//! blocks, so a profiler that reads a declared file into a buffer peaks at no less than the file's
//! size. The allocator counts every allocation in the process, so this file holds a single test.

use cassiopeia_common::{
    channel::{ChannelPolicy, channel},
    format::DataFormat,
    signal::Signal,
    telemetry::run::RunTelemetry,
};
use cassiopeia_csv_dialect::statement::DialectStatement;
use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
use cassiopeia_ir::payload::{CollectedPayload, FilePayload};
use cassiopeia_profiler::{
    passthrough_profiler::PassthroughProfiler,
    profiler::{Profiler, ProfilerRoutes},
};
use peak_alloc::PeakAlloc;
use std::{
    fs::File,
    io::{BufWriter, Write},
    sync::Arc,
};
use tempfile::tempdir;

#[global_allocator]
static ALLOCATOR: PeakAlloc = PeakAlloc;

/// The length of the file each format without a byte-level inspector is declared on. It is created
/// sparse, so it costs no disk space and reading it costs no disk I/O.
const SPARSE_LENGTH: u64 = 1 << 30;

/// The rows of the CSV file, whose dialect and encoding are inspected from its content. At 37 bytes a
/// row the file is about 70 MiB.
const CSV_ROWS: u64 = 2_000_000;

/// The most heap the profiler may add at once: well under either file, so a copy of one fails.
const HEAP_CEILING: usize = 16 << 20;

#[test]
fn a_declared_format_never_copies_its_file_onto_the_heap() {
    let directory = tempdir().unwrap();
    let sparse = directory.path().join("sparse.bin");
    File::create(&sparse).unwrap().set_len(SPARSE_LENGTH).unwrap();
    let csv = directory.path().join("stations.csv");
    let mut writer = BufWriter::new(File::create(&csv).unwrap());
    writer.write_all(b"id,name,value\n").unwrap();
    (0..CSV_ROWS).for_each(|row| writeln!(writer, "{row:08},station-{row:08},{row:08}.5").unwrap());
    writer.flush().unwrap();
    drop(writer);

    // A snapshot's header must end within its first 4 KiB, so its inspector reads no further, while
    // the rest of the file is the sparse one's length.
    let snapshot = directory.path().join("rivm.jsonl");
    let mut writer = BufWriter::new(File::create(&snapshot).unwrap());
    writeln!(
        writer,
        r#"{{"sensorthingsSnapshot":1,"serviceRoot":"https://h.example/v1.1","version":"v1.1","read":"latest","collectedAt":"2026-11-12T10:00:00Z"}}"#
    )
    .unwrap();
    writer.flush().unwrap();
    drop(writer);
    File::options().write(true).open(&snapshot).unwrap().set_len(SPARSE_LENGTH).unwrap();

    let declarations = [
        (DataFormat::Csv, &csv),
        (DataFormat::Grib, &sparse),
        (DataFormat::Json, &sparse),
        (DataFormat::GeoJson, &sparse),
        (DataFormat::Kml, &sparse),
        (DataFormat::Kmz, &sparse),
        (DataFormat::Xml, &sparse),
        (DataFormat::Shapefile, &sparse),
        (DataFormat::GeoPackage, &sparse),
        (DataFormat::NetCdf, &sparse),
        (DataFormat::SensorThings, &snapshot),
    ];

    let baseline = ALLOCATOR.current_usage();
    ALLOCATOR.reset_peak_usage();
    for (format, path) in declarations {
        let (input_tx, input_rx) = channel(ChannelPolicy::Unbounded);
        // Each payload owns its path, and the same file is declared as several formats.
        let payload = CollectedPayload::File(FilePayload::local(path.clone(), None, EncodingHint::NONE));
        input_tx.send(Signal::Data(payload)).unwrap();
        input_tx.send(Signal::Stop).unwrap();
        drop(input_tx);
        let (route_tx, route_rx) = channel(ChannelPolicy::Unbounded);
        let mut routes = ProfilerRoutes::new();
        routes.insert(format, route_tx);

        Box::new(PassthroughProfiler::new(
            format,
            DialectStatement::NOTHING,
            input_rx,
            Arc::new(RunTelemetry::new()),
        ))
        .profile(routes)
        .unwrap();

        assert!(matches!(route_rx.recv().unwrap(), Signal::Data(profiled) if *profiled.profile().format() == format));
    }
    let growth = ALLOCATOR.peak_usage().saturating_sub(baseline);

    assert!(growth < HEAP_CEILING, "the declared-format path added {growth} heap bytes at its peak");
}
