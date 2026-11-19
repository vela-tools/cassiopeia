# Changelog

## [1.3.0](https://github.com/vela-tools/cassiopeia/compare/v1.2.0...v1.3.0) - 2026-11-19

### Highlights

#### SensorThings API

**Cassiopeia can now read from OGC SensorThings API servers.** Point an input at a server's service root and set its `format` to `sensorthings`. Cassiopeia fetches the Datastreams and MultiDatastreams and attaches each reading to its Thing, location, observed property and sensor, so a mapping gets the same record shape no matter which server it reads. By default it reads the newest value of each data stream. A `range` read gets everything within a time window instead. Each read is written to a snapshot file before it is processed, and you can keep that file as a test fixture or feed it back in later as a local input. To see what a server has before writing a mapping, run `cassiopeia sensorthings readings`. Cassiopeia has been run against FROST-Server, RIVM Samen Meten, 52°North and istSOS4, and the [SensorThings API](https://github.com/vela-tools/cassiopeia/blob/v1.3.0/docs/source-formats.md#sensorthings-api) guide covers where they behave differently.

**Incremental reads pick up where the last cycle left off.** With `read: { incremental }`, a cycle asks each data stream only for the readings added since the last successful cycle, and a cursor keeps track of how far it got. The cursor only advances once the whole cycle has gone through. A scheduled job therefore sends every reading once, and a cycle that fails or gets stopped leaves the cursor alone, so the next cycle reads those readings again. Use `cassiopeia sensorthings cursor` to list, inspect or reset cursors. [Reading new readings](https://github.com/vela-tools/cassiopeia/blob/v1.3.0/docs/source-formats.md#reading-new-readings) explains the details, and [Persisting cursors](https://github.com/vela-tools/cassiopeia/blob/v1.3.0/docs/docker.md#persisting-cursors) shows how to keep cursors on a Docker volume.

**Nine new examples use real SensorThings servers.** Examples 43 to 51 in [cassiopeia-examples](https://github.com/vela-tools/cassiopeia-examples/tree/v1.3.0) all run against public servers. Among them are river gauges in Thuringia, the British Geological Survey's monitoring stations, a bicycle counter in Hamburg, a weather station on a STAplus test server, and Taiwan's Civil IoT air quality, weather, rain and flood services. Every example is a full run with its mapping, the commands, the output you should get, and an explanation of why the mapping is written that way. They begin with [a latest read](https://github.com/vela-tools/cassiopeia-examples/blob/v1.3.0/examples/43-sensorthings-latest-read/example.md) and build up through range and incremental reads, a station inventory, Things with several Locations, MultiDatastreams and HistoricalLocations. [The last one](https://github.com/vela-tools/cassiopeia-examples/blob/v1.3.0/examples/51-sensorthings-taiwan-end-to-end/example.md) keeps a broker's temporal API up to date from five Taiwanese services with a single manifest, and has to cope with sentinel values and services that no longer respond.

#### Running Cassiopeia

**Running the same inputs twice gives the same output.** When several records or inputs contributed to one entity, the value it ended up with, the order of its relationship targets and even its position in an output file could change between runs, depending on which record reached the entity store first. Cassiopeia now goes by source order, input by input as the manifest lists them and record by record within each input. Two runs of the same manifest write identical files, whichever entity store you use. See [Start with the result](https://github.com/vela-tools/cassiopeia/blob/v1.3.0/docs/mapping.md#start-with-the-result).

**Scheduled containers shut down cleanly.** `docker stop`, Kubernetes and systemd all send SIGTERM, which Cassiopeia used to ignore. A container would wait out its grace period and then get killed with exit code 137. SIGTERM and SIGHUP are now handled like Ctrl+C. The current cycle stops at its next stopping point, an atomic broker output sends nothing, no cursor moves, and the process exits with status 0. Each cycle also deletes its downloads when it finishes. Before, every cycle left a folder behind in the temporary directory. See [Stop a run](https://github.com/vela-tools/cassiopeia/blob/v1.3.0/docs/running.md#stop-a-run).

### Breaking changes

- **`yes`, `no`, `on` and `off` in a CSV field now stay text.** They used to be turned into booleans as soon as the row was read, which mangled codes that happen to be spelled that way. Norway's ISO 3166 code `NO` came out as `false`, for instance. Now only `true` and `false` become booleans. If a column really holds yes/no answers, add `transformation: "boolean"` to the attribute that reads it. That transformation still accepts all six words. See [Value inference](https://github.com/vela-tools/cassiopeia/blob/v1.3.0/docs/source-formats.md#value-inference).
- **Attributes can no longer be named after terms NGSI-LD reserves** (#134). A mapping could declare an attribute called `id`, `type`, `scope`, `expiresAt`, `observedAt`, `unitCode`, `datasetId` or another reserved name, and Cassiopeia wrote it like any other attribute. For `id`, `type`, `scope` and `expiresAt`, that left the entity with the same member twice. Such a mapping now fails to load with an error that names the attribute. Either rename it, or move `scope` and `expiresAt` to the top-level keys and `observedAt`, `unitCode` and `datasetId` into the attribute's `properties`. See [Map an attribute](https://github.com/vela-tools/cassiopeia/blob/v1.3.0/docs/mapping.md#map-an-attribute).
- **Targets of a normalized `ListRelationship` are now objects** (#126). The normalized `objectList` used to be a list of bare URI strings. ETSI GS CIM 009 v1.9.1 clause 4.5.22.2 requires each entry to be an object, so it is now written as `{ "object": "<uri>" }`. Concise and simplified output have not changed. If you read normalized files with your own code, take the URI from each entry's `object` member. See [ListRelationship](https://github.com/vela-tools/cassiopeia/blob/v1.3.0/docs/ngsi-ld/attribute-types.md#listrelationship).
- **An instance whose `datasetId` is not a URI is now dropped** (#129). Cassiopeia used to throw such a `datasetId` away without telling anyone, which turned the instance into the attribute's default instance. Now the instance itself is left out and the run warns with `extractor-dataset-id-not-a-uri`. A `datasetId` may also be any absolute URI now, not just a URN. See [Carry several instances under one name](https://github.com/vela-tools/cassiopeia/blob/v1.3.0/docs/mapping.md#carry-several-instances-under-one-name).

### Changes

#### 🚀 Features

- warn about mapping keys without effect ([#133](https://github.com/vela-tools/cassiopeia/issues/133))
- write each entity of a record on its own ([#132](https://github.com/vela-tools/cassiopeia/issues/132))
- decode json text in jsonproperty by default ([#131](https://github.com/vela-tools/cassiopeia/issues/131))
- rank each attribute and instance by its own observedat as an instant ([#128](https://github.com/vela-tools/cassiopeia/issues/128), [#141](https://github.com/vela-tools/cassiopeia/issues/141))
- warn when a sensorthings read gets observations without their feature of interest ([#171](https://github.com/vela-tools/cassiopeia/issues/171))
- read the times and observed area of each sensorthings data stream ([#167](https://github.com/vela-tools/cassiopeia/issues/167))
- read sensorthings things that have no data streams ([#166](https://github.com/vela-tools/cassiopeia/issues/166))
- read where a sensorthings thing has been from its historical locations ([#163](https://github.com/vela-tools/cassiopeia/issues/163))
- read every location of a sensorthings thing ([#162](https://github.com/vela-tools/cassiopeia/issues/162))
- read sensorthings multidatastreams ([#156](https://github.com/vela-tools/cassiopeia/issues/156))
- read the sensor of each sensorthings data stream ([#155](https://github.com/vela-tools/cassiopeia/issues/155))
- read where each sensorthings reading was taken from its feature of interest ([#154](https://github.com/vela-tools/cassiopeia/issues/154))
- inspect and reset sensorthings cursors from the command line ([#102](https://github.com/vela-tools/cassiopeia/issues/102))
- read new sensorthings readings incrementally through a cursor ([#101](https://github.com/vela-tools/cassiopeia/issues/101))
- keep sensorthings read positions in a persistent cursor store ([#106](https://github.com/vela-tools/cassiopeia/issues/106), [#100](https://github.com/vela-tools/cassiopeia/issues/100))
- read a time range of readings from a sensorthings api server ([#99](https://github.com/vela-tools/cassiopeia/issues/99))
- list the readings a sensorthings api server offers ([#98](https://github.com/vela-tools/cassiopeia/issues/98))
- read the latest readings from a sensorthings api server ([#107](https://github.com/vela-tools/cassiopeia/issues/107), [#97](https://github.com/vela-tools/cassiopeia/issues/97))
- read sensorthings snapshot files and refuse range snapshots without series output ([#105](https://github.com/vela-tools/cassiopeia/issues/105), [#96](https://github.com/vela-tools/cassiopeia/issues/96))
- add cefact_unit template filter ([#93](https://github.com/vela-tools/cassiopeia/issues/93))

#### 🐛 Bug Fixes

- report the collector to profiler channel once in the run summary
- show the root cause beside the outermost one in concise diagnostics
- count schema warnings of a series run per entity, not per record
- list cursors only once the run has stopped
- judge polygon winding in the longitude/latitude plane
- retry downloads that fail in transport and stop a cycle stopped during its retry wait
- read only true and false as booleans in csv fields
- take a shared attribute name whole from the first mapping that gives it a value
- **cli:** say what manifest generate records
- give each synthetic entity one mapping id per run
- sort redb ids for hand-off in bounded runs ([#178](https://github.com/vela-tools/cassiopeia/issues/178))
- treat a stop in a retry wait or broker delivery as no error ([#152](https://github.com/vela-tools/cassiopeia/issues/152))
- watch stop signals only in map and readings ([#150](https://github.com/vela-tools/cassiopeia/issues/150))
- report the failure of the first input listed
- merge shared-mapping inputs in manifest order ([#181](https://github.com/vela-tools/cassiopeia/issues/181))
- keep relationship targets in source order ([#180](https://github.com/vela-tools/cassiopeia/issues/180))
- remove each cycle's downloads when it ends ([#151](https://github.com/vela-tools/cassiopeia/issues/151))
- stop on sigterm, report stopped cycles ([#150](https://github.com/vela-tools/cassiopeia/issues/150), [#152](https://github.com/vela-tools/cassiopeia/issues/152))
- refuse attributes named after reserved names ([#134](https://github.com/vela-tools/cassiopeia/issues/134))
- count each reason a record is skipped under its own code ([#130](https://github.com/vela-tools/cassiopeia/issues/130))
- write any absolute uri as a datasetid and drop the instance whose datasetid is no uri with a warning ([#129](https://github.com/vela-tools/cassiopeia/issues/129))
- write the entities of a current-state file in the order of their first records so every run and store writes the same files ([#178](https://github.com/vela-tools/cassiopeia/issues/178))
- keep each entity's records in source order so repeated runs keep the same values and write the same series files ([#170](https://github.com/vela-tools/cassiopeia/issues/170), [#177](https://github.com/vela-tools/cassiopeia/issues/177))
- write each normalized list relationship target as an object holding its uri under object ([#126](https://github.com/vela-tools/cassiopeia/issues/126))
- bundle the data integrity context the core context imports so value type expansion stays offline
- write the page size of a sensorthings discovery before its expansion ([#169](https://github.com/vela-tools/cassiopeia/issues/169))
- check the members a schema requires on the entity a sensorthings series run writes ([#168](https://github.com/vela-tools/cassiopeia/issues/168))
- **cli:** align manifest generate flags with map and correct temporal output docs ([#95](https://github.com/vela-tools/cassiopeia/issues/95))
- report a unitcode that is not a un/cefact code ([#94](https://github.com/vela-tools/cassiopeia/issues/94))

#### ⚡ Performance

- sort redb ids for hand-off through extsort run files
- select only the observation members a sensorthings read uses ([#165](https://github.com/vela-tools/cassiopeia/issues/165))

<details>
<summary>Internal changes (33)</summary>

#### 🔧 Refactor

- read relationships at assembly through one consuming store method
- rework attribute builders

#### 📚 Documentation

- name sensorthings, stop signals and contexts
- describe source-order merging in resolver and report comments
- complete the sensorthings server differences the release check found
- describe how 52north and istsos4 answer sensorthings reads ([#169](https://github.com/vela-tools/cassiopeia/issues/169))
- update contributing documentation
- fix outdated links

#### 📦 Dependencies

- bump netcdf-writer from 0.12.1 to 0.13.0
- bump sha2 from 0.10.9 to 0.11.1
- bump netcdf-core from 0.12.1 to 0.13.0
- bump the cargo-minor group across 1 directory with 5 updates
- bump netcdf-reader from 0.12.1 to 0.13.0
- bump lz4_flex from 0.11.6 to 0.11.7
- bump the cargo-minor group across 1 directory with 13 updates
- bump rust
- bump rust-toolchain from 1.99.1 to 1.100.0
- bump distroless/cc-debian13 from `e1f5543` to `809d04a`

#### 🏗️ Build

- ignore unmaintained chrono and humantime advisories for time being
- adopt final changelog and tag format

#### 👷 CI

- fetch libaec from its github mirror while gitlab.dkrz.de is offline
- quote the golden test filter and format the ngsi-ld manifest

#### 🧪 Tests

- prove series runs keep unobserved values from the first record ([#170](https://github.com/vela-tools/cassiopeia/issues/170))
- cover a newest record without a value under a shared observedat ([#141](https://github.com/vela-tools/cassiopeia/issues/141))
- extend golden corpus, name first difference ([#127](https://github.com/vela-tools/cassiopeia/issues/127))
- add a golden corpus that snapshots what each v4 mapping construct writes, checked in its own workflow
- check the first top of a sensorthings latest request with and without a filter ([#169](https://github.com/vela-tools/cassiopeia/issues/169))
- import yore code_pages module for clippy absolute_paths on rust 1.100

#### 🧹 Chores

- bump version
- update dependencies
- update dependencies
- update dependabot configuration
- update issue templates

</details>

## [1.2.0](https://github.com/vela-tools/cassiopeia/compare/v1.1.1...v1.2.0) - 2026-11-11

### Highlights

**Two new source formats: GeoPackage and NetCDF.**

- [GeoPackage](https://github.com/vela-tools/cassiopeia/blob/v1.2.0/docs/source-formats.md#geopackage) (`geopackage`) reads every features and attributes table of an OGC GeoPackage as one record per row, with geometries reprojected to WGS 84 and each table routed to its own collection when a file has several. A table that cannot be read, such as a view, is skipped with a warning instead of stopping the run. GeoPackage support needs SQLite, so it is part of the default source build and the Docker image but not the release binaries; see Security below.
- [NetCDF](https://github.com/vela-tools/cassiopeia/blob/v1.2.0/docs/source-formats.md#netcdf) (`netcdf`) reads the classic CDF-1, CDF-2 and CDF-5 formats and NetCDF-4 on HDF5 in pure Rust, so every build has it, the release binaries included. A gridded file, as Copernicus CAMS, CMEMS and ERA5 publish them, gives the same wide location records as GRIB, so one mapping style serves both formats. A CF `timeSeries` station file gives one record per observation in all five CF layouts, and times in non-Gregorian calendars are written as the right Gregorian dates.

**CSV that copes with real-world files.** Cassiopeia now detects a file's character encoding, including byte order marks, spreadsheet UTF-16 exports without one, Windows-1252 and ISO-8859-2, and a server's declared `charset`. It skips title rows above the table, joins stacked header rows into one name per column, skips a row of another width with a warning instead of misreading it, and reads backslash-escaped quotes and CR line endings. When detection still gets a file wrong, a manifest input can state its [`dialect`](https://github.com/vela-tools/cassiopeia/blob/v1.2.0/docs/manifest.md#csv-dialect), and `cassiopeia profile` prints the detected dialect in that same syntax.

**JSON beyond a top-level array.** An input with a `mappings` list reads arrays of objects nested at any depth as collections, and a lone object on a collection path is read as one item. Each GeoJSON feature's record now carries its `FeatureCollection`'s foreign members and `bbox` under `featureCollection`.

**Authenticated, resilient downloads.** A remote input can send its own HTTP [`headers`](https://github.com/vela-tools/cassiopeia/blob/v1.2.0/docs/manifest.md#per-input-headers), or `--input-header` on the command line, for an API key. A manifest reads [environment variables](https://github.com/vela-tools/cassiopeia/blob/v1.2.0/docs/manifest.md#environment-variables) as `${NAME}`, so credentials stay out of the file. A source that answers `429` or a `5xx` is retried with backoff, honouring `Retry-After`.

**Richer, stricter NGSI-LD output.**

- An entity can carry an [`expiresAt`](https://github.com/vela-tools/cassiopeia/blob/v1.2.0/docs/mapping.md#set-an-expiry).
- An attribute can [declare its `valueType`](https://github.com/vela-tools/cassiopeia/blob/v1.2.0/docs/mapping.md#declare-a-values-datatype), and `datetime`, `date` and `time` values are typed. `datetime` reads many more real-world spellings, such as basic-format offsets, RFC 9557 zoned timestamps and Unix epochs, and keeps at most the six fractional digits NGSI-LD admits.
- Entities never carry JSON `null`, which NGSI-LD does not allow.

**New transformations.** `iri` and `urn` read identifiers, and `json` reads JSON text as the value it encodes. A `ListProperty` reads each item with its declared transformation.

**Mappings fail fast.** More mistakes stop a mapping when it loads, naming the attribute, instead of failing on every record: a transformation the attribute type cannot use, `instances` below a top-level attribute, an illegal literal scope, and a misplaced `valueType`. Warnings about a missing field or a failing template now name the field and the attribute by their full path.

### Breaking changes

- **Relative paths in a manifest resolve next to the manifest** (#46), not against the directory Cassiopeia is started from. A manifest whose paths were written relative to the working directory needs them rewritten relative to the manifest file, or made absolute. Paths given on the command line still resolve against the working directory.
- **The null-handling options are gone.** `--writer-skip-null` and `--validation-skip-null` on `cassiopeia map`, `--skip-null` and `--validation-skip-null` on `cassiopeia manifest generate`, and the manifest's `output.skipNull` and `output.validation.skipNull` are removed; delete them from scripts and manifests. Entities never carry JSON `null` (#77): a `null` inside an array or object is left out with a warning, and an attribute whose value is only `null` is dropped. An attribute whose source holds empty text is now written as `""`, where the old default left it out.
- **`--header` is now `--output-header`.** It still sets headers on Context Broker requests; the new `--input-header` sets them on input downloads.
- **A transformation an attribute type cannot use stops the mapping from loading**, naming the attribute, where it used to fail on every record or be ignored. A `Relationship` or `ListRelationship` takes no transformation at all.
- **A `VocabProperty` value must be an absolute IRI.** A value that is not is dropped with an `extractor-iri-unreadable` warning; `transformation: "urn"` admits only a URN.
- **`object` and `array` read JSON text** (#52): text such as `[1,2]` now becomes the array it encodes. A `JsonProperty` whose JSON arrives as text, as every CSV field does, needs `transformation: "json"`. A `JsonProperty` keeps any `null` inside its raw JSON as the source has it, and refuses a value that is not an object or an array of objects.
- **A transformation on an attribute built from nested `mappings` is applied** to the object they build (#61), where it used to be ignored unless it was a geometry transformation.
- **A failing `Relationship` source costs only that relationship** (#54): the entity is still written, with a warning, where the whole record used to be skipped.
- **A geometry outside WGS 84 is refused** (#66). An attribute whose coordinates fall outside longitude ±180 or latitude ±90, as projected coordinates do, is now dropped with a warning naming it, where it used to be written.
- **The Docker image has no shell.** Its runtime is now distroless, so `docker exec` into a shell no longer works; run `cassiopeia` subcommands through `docker run`. The image is now also tagged without the `v` prefix, as `1.2.0`.
- **Building GRIB1 support from source needs different tools.** The build compiles a bundled ecCodes and no longer uses a system one: it needs CMake, a C and C++ compiler, `git`, `libclang` and OpenJPEG. See [Build tools for GRIB1](https://github.com/vela-tools/cassiopeia/blob/v1.2.0/docs/getting-started.md#build-tools-for-grib1).

### Security

**SQLite write-ahead logs in GeoPackages: [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459), [CVE-2026-119375](https://nvd.nist.gov/vuln/detail/CVE-2026-119375), [GHSA-72w2-rjgg-q729](https://github.com/advisories/GHSA-72w2-rjgg-q729). Medium severity (CVSS 4.0: 6.8). Mitigated in this release.**

SQLite before 3.54.0 can return leftover bytes of the process's memory as ordinary row data when it opens a database together with a crafted write-ahead log, a `-wal` file beside the database, whose page size differs from the database's. Cassiopeia reads GeoPackages through SQLite, so such a pair of files could make data from Cassiopeia's own memory, which can include other inputs or broker credentials, appear in the entities it writes to files or sends to a context broker. Exploiting it requires someone who can supply both files to a run; the leaked bytes then reach whoever can read the run's output.

GeoPackage support is new in v1.2.0, so no earlier release is affected.

**The release binaries and the Docker image are not affected.**

| How you got Cassiopeia | Affected |
| --- | --- |
| A release binary from this page | No. The release binaries are built without GeoPackage support and contain no SQLite. |
| The Docker image `ghcr.io/vela-tools/cassiopeia:v1.2.0` | No. It links SQLite 3.54.1, which carries the fix, and its new distroless base contains no other SQLite. |
| Built from source without the `geopackage` feature | No. The build contains no SQLite. |
| Built from source against a system SQLite: `geopackage` without `geopackage-bundled` | No, as long as the system SQLite is 3.54.0 or newer, or your distribution has backported the fix to it. |
| Built from source with the bundled SQLite: the default features, as `cargo install` and `cargo build` use | Only in the narrow case described below. |

Run `cassiopeia --version` to see which build you have. Its `features` line shows `-geopackage` for a build without GeoPackage support, `+geopackage · -geopackage-bundled` for a build linked against a system SQLite, as the Docker image is, and `+geopackage · +geopackage-bundled` for a build with the bundled SQLite.

**How it is mitigated.** Every build now refuses a GeoPackage that has a write-ahead log beside it, before SQLite opens the file, and the error names the log. SQLite reads such a log whenever one exists, whatever the GeoPackage itself declares, so the presence of the log is what Cassiopeia refuses. Cassiopeia also hands SQLite the GeoPackage's resolved absolute path, so a symbolic link cannot point SQLite at a log the check did not see. A legitimate log is usually left by a program that still has the GeoPackage open, such as QGIS: close that program and run again. See [Write-ahead logs](https://github.com/vela-tools/cassiopeia/blob/v1.2.0/docs/source-formats.md#write-ahead-logs).

**What remains for builds with the bundled SQLite.** The bundled SQLite is still 3.53.2, because the crates Cassiopeia reads GeoPackages through do not yet bundle 3.54. The refusal covers a log that is present when Cassiopeia opens the GeoPackage, but SQLite looks for the log again at the start of every read transaction. Someone who can write to the GeoPackage's directory while a run is in progress could therefore add a log after the check. Only a patched SQLite closes that gap. If an untrusted party can write to your input directories while Cassiopeia runs, use the Docker image, or build against a system SQLite that carries the fix, as [GeoPackage and SQLite](https://github.com/vela-tools/cassiopeia/blob/v1.2.0/docs/getting-started.md#geopackage-and-sqlite) describes. Check your distribution first: at the time of this release, neither Debian 13 nor Ubuntu 26.04 has shipped a fixed SQLite. A later release will update the bundled SQLite once the upstream crates ship it, and the temporary `geopackage-bundled` feature will be removed then.

**Scanner reports.** The Docker image's binary is now built with `cargo auditable`, so `cargo audit bin` and container scanners such as Trivy and Grype can see the Rust crates inside it. They report RUSTSEC-2026-0459 against the `libsqlite3-sys` crate. That advisory covers the SQLite the crate bundles, which the image does not compile in, so the report does not apply to the image. It goes away with the bundled SQLite update.

### Changes

#### ⚠️ Breaking changes

- add the iri and urn transformations and read a vocab property as an iri
- keep a json property's nulls and warn when a value holds only nulls
- refuse a transformation an attribute type cannot use when the mapping loads
- keep the entity when a relationship source template fails ([#54](https://github.com/vela-tools/cassiopeia/issues/54))
- apply the declared transformation to an object built from nested mappings ([#61](https://github.com/vela-tools/cassiopeia/issues/61))
- read json text in object, array and a new json transformation ([#52](https://github.com/vela-tools/cassiopeia/issues/52))
- resolve a manifest's relative paths next to the manifest ([#46](https://github.com/vela-tools/cassiopeia/issues/46))
- never write json null in an entity ([#77](https://github.com/vela-tools/cassiopeia/issues/77))

#### 🔒 Security

- run the docker image on distroless so it ships no vulnerable system sqlite
- link the docker image against sqlite 3.54.1 built from source instead of the bundled copy
- add a geopackage-bundled feature so a build can link the system sqlite
- refuse a geopackage with a write-ahead log beside it (RUSTSEC-2026-0459)

#### 🚀 Features

- write single-precision netcdf and grib values as their shortest decimal ([#92](https://github.com/vela-tools/cassiopeia/issues/92))
- ingest netcdf grids and cf timeseries station files
- declare the valuetype of an attribute in a mapping ([#48](https://github.com/vela-tools/cassiopeia/issues/48))
- ingest geopackage feature and attribute tables
- refuse a geometry whose coordinates are outside wgs 84 ([#66](https://github.com/vela-tools/cassiopeia/issues/66))
- keep a geometry collection on a property under the geometry transformation
- read each item of a list property with its declared transformation
- read featurecollection members from each geojson feature's record ([#42](https://github.com/vela-tools/cassiopeia/issues/42))
- read a lone json object on a listed collection path as one item ([#34](https://github.com/vela-tools/cassiopeia/issues/34))
- map json records nested below the top level as collections ([#34](https://github.com/vela-tools/cassiopeia/issues/34))
- retry a source that answers 429 or 5xx ([#40](https://github.com/vela-tools/cassiopeia/issues/40))
- read environment variables in a manifest ([#36](https://github.com/vela-tools/cassiopeia/issues/36))
- send request headers when fetching an input ([#35](https://github.com/vela-tools/cassiopeia/issues/35))
- let a manifest input state its csv dialect ([#38](https://github.com/vela-tools/cassiopeia/issues/38))
- join stacked csv header rows into one field name per column ([#37](https://github.com/vela-tools/cassiopeia/issues/37))
- read iso 8601 basic and hour-only offsets in datetime ([#49](https://github.com/vela-tools/cassiopeia/issues/49))
- emit an entity-level expiresat member ([#39](https://github.com/vela-tools/cassiopeia/issues/39))

#### 🐛 Bug Fixes

- write netcdf times in real calendars as gregorian dates ([#91](https://github.com/vela-tools/cassiopeia/issues/91))
- keep the source degrees of a coordinate that reprojection does not move ([#90](https://github.com/vela-tools/cassiopeia/issues/90))
- skip a geopackage table that cannot be read instead of stopping the run ([#89](https://github.com/vela-tools/cassiopeia/issues/89))
- name the surface a netcdf depth is measured below from its data ([#88](https://github.com/vela-tools/cassiopeia/issues/88))
- keep run variables out of the record ([#87](https://github.com/vela-tools/cassiopeia/issues/87))
- read no more of a declared input than its inspector touches ([#86](https://github.com/vela-tools/cassiopeia/issues/86))
- reconcile a broker echo whose urn namespace differs in case
- read any json-ld 1.1 local `@context` ([#50](https://github.com/vela-tools/cassiopeia/issues/50))
- give one scope per item when a scope template reads a list whole ([#80](https://github.com/vela-tools/cassiopeia/issues/80))
- refuse a scope template that reads a boolean ([#79](https://github.com/vela-tools/cassiopeia/issues/79))
- refuse urn:ngsi-ld:null as the scope of a written entity ([#78](https://github.com/vela-tools/cassiopeia/issues/78))
- describe ngsi-ld scope and name rules by unicode letters and numbers, not digits
- give the missing-field hint for a nested field the record lacks, named down to its first missing part ([#56](https://github.com/vela-tools/cassiopeia/issues/56))
- name a failing sub-attribute template by its path from the top-level attribute
- give the missing-field hint when a template prints a field the record lacks
- give no value for a list source when none of the fields it reads is present ([#53](https://github.com/vela-tools/cassiopeia/issues/53))
- leave blank items out of a list gathered from an array, so a split's empty pieces vanish ([#59](https://github.com/vela-tools/cassiopeia/issues/59))
- name a refused geometry by its attribute path and report each declaration apart ([#62](https://github.com/vela-tools/cassiopeia/issues/62))
- refuse instances declared below a top-level attribute when the mapping loads ([#63](https://github.com/vela-tools/cassiopeia/issues/63))
- offer a wizard container only the attribute types its locked object conversion loads as ([#64](https://github.com/vela-tools/cassiopeia/issues/64))
- warn when a boolean, integer or float value cannot be read
- warn when a geo property value spells no geometry
- warn when an object transformation gets a value that is no object
- link each merged relationship target once in first-seen order ([#45](https://github.com/vela-tools/cassiopeia/issues/45))
- warn when a vocab or language property value cannot be used ([#65](https://github.com/vela-tools/cassiopeia/issues/65))
- make non-finite floats unrepresentable in value
- keep language map and vocab members in concise and simplified output
- reject unknown keys in temporal output configuration
- add missing license to allowlist
- download http/2 sources whose gzip bodies carry empty data frames ([#41](https://github.com/vela-tools/cassiopeia/issues/41))
- read csv rows whose only extra or missing fields come from a trailing delimiter
- recognise bomless utf-16 csv files correctly
- fail the profile report when the csv dialect cannot be serialised
- count each renamed csv column in the run's warning total
- name csv sources in errors and warnings and harden dialect and encoding checks
- keep csv data rows with blanks or missing markers out of the header
- detect cr line endings when a quoted cell holds a line feed
- detect a csv header over a single data row
- report a csv's bom encoding over a stated one
- keep csv warnings about different files apart
- detect csv files with cr line endings, one column, or bom-less utf-16
- drop the padding around csv header names
- skip csv rows of another width with a warning
- report a csv's encoding from its byte order mark
- cap the csv renamed-columns headline and list every rename in verbose
- read backslash-escaped csv quotes, detected and stated
- keep csv columns whose header keys collide
- cut the csv dialect sample at a record, not a line
- detect csv encoding from the whole file and reject bytes it cannot decode
- skip csv title rows as wide as the table
- read a remote csv's encoding from its charset and domain
- skip title rows above a csv table as a record-counted preamble ([#37](https://github.com/vela-tools/cassiopeia/issues/37))
- report records discarded for a failing observedat template ([#55](https://github.com/vela-tools/cassiopeia/issues/55))
- type time values with valuetype and cap fractions at six digits ([#43](https://github.com/vela-tools/cassiopeia/issues/43), [#47](https://github.com/vela-tools/cassiopeia/issues/47))
- read rfc 9557 zoned timestamps in datetime ([#44](https://github.com/vela-tools/cassiopeia/issues/44))
- omit entity scope and expiresat from the simplified representation ([#39](https://github.com/vela-tools/cassiopeia/issues/39))
- merge entity expiresat per clause 4.5.5.3 and bound its warnings ([#39](https://github.com/vela-tools/cassiopeia/issues/39))
- emit expiresat as an ngsi-ld qualifier ([#39](https://github.com/vela-tools/cassiopeia/issues/39))

This list leaves out 35 internal changes. The [commit history](https://github.com/vela-tools/cassiopeia/compare/v1.1.1...v1.2.0) has them.

## [1.1.1](https://github.com/vela-tools/cassiopeia/compare/v1.1.0...v1.1.1) - 2026-10-28

### Changes

#### 🐛 Bug Fixes

- clear clippy warnings under rust 1.99.1

This list leaves out 10 internal changes. The [commit history](https://github.com/vela-tools/cassiopeia/compare/v1.1.0...v1.1.1) has them.

## [1.1.0](https://github.com/vela-tools/cassiopeia/compare/v1.0.3...v1.1.0) - 2026-10-02

### Changes

#### ⚠️ Breaking changes

- report every dropped geoproperty and honour its geometry policy at every level. a GeoProperty whose transformation does not produce a
  geometry (`string`, `integer`, `object`, ...) is now refused when the
  mapping loads; drop the transformation, since the default `geometry`
  already reads GeoJSON objects and GeoJSON text.
- convert a geoproperty without a transformation as a geometry. a `geometry` block on an attribute whose value is not
  converted to a geometry is now refused when the mapping loads, instead of
  being silently ignored.
- map a constant identity to one entity and warn when merged records disagree. a mapping whose identity has no template no longer gets
  numbered URNs (`Type-1`, `Type-2`, …); every record maps to the one entity
  the identity names, and records that disagree on a field the mapping reads
  raise a `resolver-records-merged` warning.
- move scope out of identity to the top level of a mapping. a mapping that declares `identity.scope` or an unknown
  top-level key no longer loads. Move `scope` out of `identity` to the
  top level of the mapping and remove any other unknown top-level key.

#### 🚀 Features

- drop only the attribute when its template fails
- keep the type of a single template expression
- temporal properties in kml

#### 🐛 Bug Fixes

- keep the winding of a geometry its geometry policy already normalised
- keep the structure of list and json properties declared without a transformation
- read numeric unix epochs and report unreadable non-text timestamps
- give every template failure a located, actionable message
- route every non-plain template through tera
- resolve a composite template with a missing field to nothing
- reject a mapping whose template does not compile
- treat separator-only identifiers as no target
- carry relationship instance grouping and drop unmintable list tokens
- render arrays and objects as compact text
- array/object transformations stop treating blanks and text as values
- do not emit relationships to null synthetic entities
- union scopes per entity and drop scopes with missing fields

This list leaves out 6 internal changes. The [commit history](https://github.com/vela-tools/cassiopeia/compare/v1.0.3...v1.1.0) has them.

## [1.0.3](https://github.com/vela-tools/cassiopeia/compare/v1.0.2...v1.0.3) - 2026-09-17

### Changes

#### 🐛 Bug Fixes

- eliminate a bug in csv header detection

This list leaves out 3 internal changes. The [commit history](https://github.com/vela-tools/cassiopeia/compare/v1.0.2...v1.0.3) has them.

## [1.0.2](https://github.com/vela-tools/cassiopeia/compare/v1.0.1...v1.0.2) - 2026-09-09

### Changes

This list leaves out 3 internal changes. The [commit history](https://github.com/vela-tools/cassiopeia/compare/v1.0.1...v1.0.2) has them.

## [1.0.1](https://github.com/vela-tools/cassiopeia/compare/v1.0.0...v1.0.1) - 2026-09-08

### Changes

This list leaves out 24 internal changes. The [commit history](https://github.com/vela-tools/cassiopeia/compare/v1.0.0...v1.0.1) has them.

## [1.0.0](https://github.com/vela-tools/cassiopeia/releases/tag/v1.0.0) - 2026-09-07

Initial release.

<!-- generated by git-cliff -->
