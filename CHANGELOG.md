# Changelog

All notable changes to this project will be documented in this file.

## [v1.2.0] - 2026-11-11

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

#### 🏗️ Build
- make the docker image binary auditable and fail the image build on a vulnerable crate
- `deps` — bump sigstore/cosign-installer
- modify changelog template
- move to iri-rs 4 and jsonld 0.27

#### 🐛 Bug Fixes
- write netcdf times in real calendars as gregorian dates
- keep the source degrees of a coordinate that reprojection does not move
- skip a geopackage table that cannot be read instead of stopping the run
- name the surface a netcdf depth is measured below from its data
- keep run variables out of the record
- read no more of a declared input than its inspector touches
- reconcile a broker echo whose urn namespace differs in case
- read any json-ld 1.1 local @context
- give one scope per item when a scope template reads a list whole
- refuse a scope template that reads a boolean
- refuse urn:ngsi-ld:null as the scope of a written entity
- describe ngsi-ld scope and name rules by unicode letters and numbers, not digits
- give the missing-field hint for a nested field the record lacks, named down to its first missing part
- name a failing sub-attribute template by its path from the top-level attribute
- give the missing-field hint when a template prints a field the record lacks
- give no value for a list source when none of the fields it reads is present
- leave blank items out of a list gathered from an array, so a split's empty pieces vanish
- name a refused geometry by its attribute path and report each declaration apart
- refuse instances declared below a top-level attribute when the mapping loads
- offer a wizard container only the attribute types its locked object conversion loads as
- keep a json property's nulls and warn when a value holds only nulls ⚠️ **BREAKING**
- warn when a boolean, integer or float value cannot be read
- keep the entity when a relationship source template fails ⚠️ **BREAKING**
- warn when a geo property value spells no geometry
- apply the declared transformation to an object built from nested mappings ⚠️ **BREAKING**
- warn when an object transformation gets a value that is no object
- link each merged relationship target once in first-seen order
- warn when a vocab or language property value cannot be used
- make non-finite floats unrepresentable in value
- keep language map and vocab members in concise and simplified output
- reject unknown keys in temporal output configuration
- never write json null in an entity ⚠️ **BREAKING**
- add missing license to allowlist
- download http/2 sources whose gzip bodies carry empty data frames
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
- skip title rows above a csv table as a record-counted preamble
- report records discarded for a failing observedat template
- type time values with valuetype and cap fractions at six digits
- read rfc 9557 zoned timestamps in datetime
- omit entity scope and expiresat from the simplified representation
- merge entity expiresat per clause 4.5.5.3 and bound its warnings
- emit expiresat as an ngsi-ld qualifier

#### 👷 CI
- fail a release whose binary was built with geopackage
- also tag the image without the v prefix

#### 📚 Documentation
- fix link
- update contacts & security policy
- list the csv renamed-columns warning and the preamble detection limit
- say blank lines are not csv records and tell rows from lines
- warn that csv footer rows must be removed before ingesting
- note that the leftmost repeated csv column now keeps the key
- cite the clauses that define expiresat and observedat
- describe every record shape the json ingestor accepts

#### 🔒 Security
- run the docker image on distroless so it ships no vulnerable system sqlite
- link the docker image against sqlite 3.54.1 built from source instead of the bundled copy
- add a geopackage-bundled feature so a build can link the system sqlite
- refuse a geopackage with a write-ahead log beside it (RUSTSEC-2026-0459)

#### 🔧 Refactor
- code layout overhaul
- say why a read value is absent and keep a parsed identifier as its type
- share one blank check, one admission outcome, and one refusal per reading across value conversion
- convert each attribute type through one trait and resolve every declaration through one walk
- batch every ingestor's records through the shared record batches
- move encoding detection to its own crate and split the csv ingestor and inspector
- name the entity expiry codes after expiresat and trim qualifier allocations
- share one batch sink across stage loss reports

#### 🚀 Features
- write single-precision netcdf and grib values as their shortest decimal
- ingest netcdf grids and cf timeseries station files
- declare the valuetype of an attribute in a mapping
- ingest geopackage feature and attribute tables
- refuse a geometry whose coordinates are outside wgs 84
- add the iri and urn transformations and read a vocab property as an iri ⚠️ **BREAKING**
- keep a geometry collection on a property under the geometry transformation
- read each item of a list property with its declared transformation
- refuse a transformation an attribute type cannot use when the mapping loads ⚠️ **BREAKING**
- read featurecollection members from each geojson feature's record
- read a lone json object on a listed collection path as one item
- map json records nested below the top level as collections
- read json text in object, array and a new json transformation ⚠️ **BREAKING**
- resolve a manifest's relative paths next to the manifest ⚠️ **BREAKING**
- retry a source that answers 429 or 5xx
- read environment variables in a manifest
- send request headers when fetching an input
- let a manifest input state its csv dialect
- join stacked csv header rows into one field name per column
- read iso 8601 basic and hour-only offsets in datetime
- emit an entity-level expiresat member

#### 🧪 Tests
- prove an illegal literal scope fails the mapping load
- prove a geo property warns on a value that is no geometry end to end
- split the pipeline end-to-end scenarios out of the pipeline module
- cover csv preambles whose records span several lines
- ingest a quoted two-line csv title with the inspected dialect

#### 🧹 Chores
- update dependencies
- update cliff configuration
- bump version
- update dependencies
- update dependencies
- exclude target dir for taplo
- update dependencies and vendor eccodes
- update dependencies

## [v1.1.1] - 2026-10-28

### Changes

#### 🏗️ Build
- bump docker base images to rust 1.99.1 and trixie-20261027

#### 🐛 Bug Fixes
- clear clippy warnings under rust 1.99.1

#### 👷 CI
- check the msrv with cargo msrv every two weeks
- pin the rust toolchain and let dependabot bump it

#### 📚 Documentation
- state rust 1.96.1 as the minimum
- add clippy and cargo deny badges to the readme

#### 🧪 Tests
- keep reporter and rendering tests independent of the terminal
- use pretty_assertions across the workspace

#### 🧹 Chores
- bump version
- adjust msrv
- update dependencies

## [v1.1.0] - 2026-10-02

### Changes

#### 🏗️ Build
- add cargo machete override

#### 🐛 Bug Fixes
- report every dropped geoproperty and honour its geometry policy at every level ⚠️ **BREAKING**
- keep the winding of a geometry its geometry policy already normalised
- convert a geoproperty without a transformation as a geometry ⚠️ **BREAKING**
- keep the structure of list and json properties declared without a transformation
- map a constant identity to one entity and warn when merged records disagree ⚠️ **BREAKING**
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
- move scope out of identity to the top level of a mapping ⚠️ **BREAKING**

#### 📚 Documentation
- document template, identity and scope behaviour since v1.0.3
- move worked examples to the examples repository

#### 🔧 Refactor
- split attribute assembly out of the ngsi-ld transformer

#### 🚀 Features
- drop only the attribute when its template fails
- keep the type of a single template expression
- temporal properties in kml

#### 🧹 Chores
- update dependencies
- bump version

## [v1.0.3] - 2026-09-17

### Changes

#### 🐛 Bug Fixes
- eliminate a bug in csv header detection

#### 👷 CI
- add pages workflow

#### 🧹 Chores
- bump version
- update dependencies

## [v1.0.2] - 2026-09-09

### Changes

#### 🔧 Refactor
- unify user agent handling on both collector and writer

#### 🧹 Chores
- update changelog
- bump version

## [v1.0.1] - 2026-09-08

### Changes

#### 🏗️ Build
- no need to depend on eccodes dev package in docker image
- add missing package to docker image
- use locked build with cargo chef

#### 👷 CI
- use correct cosign installer version
- add static musl builds for linux
- add missing workflow permissions
- add taplo ci gate
- add container image attestation
- add release matrix table
- add build provenance attestation
- ship hashes as artifacts
- integrate cosign into image building process
- use composite action for native dependencies

#### 📚 Documentation
- switch to static musl builds as default
- update prebuilt binary instructions
- document container image verification
- fix incorrect link in docs
- better readme header

#### 🧹 Chores
- update changelog
- update changelog
- add .gitattributes
- update dependencies
- bump version
- reconfigure taplo

## [v1.0.0] - 2026-09-07

### Changes

#### 🚀 Features
- first cassiopeia version

#### 🧹 Chores
- init

<!-- generated by git-cliff -->
