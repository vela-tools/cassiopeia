# Command-Line Help for `cassiopeia`

This document contains the help content for the `cassiopeia` command-line program.

**Command Overview:**

* [`cassiopeia`↴](#cassiopeia)
* [`cassiopeia sdm`↴](#cassiopeia-sdm)
* [`cassiopeia sdm download`↴](#cassiopeia-sdm-download)
* [`cassiopeia sdm list`↴](#cassiopeia-sdm-list)
* [`cassiopeia sdm search`↴](#cassiopeia-sdm-search)
* [`cassiopeia mapping`↴](#cassiopeia-mapping)
* [`cassiopeia mapping format`↴](#cassiopeia-mapping-format)
* [`cassiopeia mapping convert`↴](#cassiopeia-mapping-convert)
* [`cassiopeia schema`↴](#cassiopeia-schema)
* [`cassiopeia schema dereference`↴](#cassiopeia-schema-dereference)
* [`cassiopeia manifest`↴](#cassiopeia-manifest)
* [`cassiopeia manifest generate`↴](#cassiopeia-manifest-generate)
* [`cassiopeia config`↴](#cassiopeia-config)
* [`cassiopeia config generate`↴](#cassiopeia-config-generate)
* [`cassiopeia map`↴](#cassiopeia-map)
* [`cassiopeia explorer`↴](#cassiopeia-explorer)
* [`cassiopeia wizard`↴](#cassiopeia-wizard)
* [`cassiopeia profile`↴](#cassiopeia-profile)
* [`cassiopeia bugreport`↴](#cassiopeia-bugreport)

## `cassiopeia`

Map and transform source data into standards-compliant NGSI-LD entities.

**Usage:** `cassiopeia [OPTIONS] <COMMAND>`

###### **Subcommands:**

* `sdm` — Smart Data Models operations
* `mapping` — Mapping file operations
* `schema` — JSON Schema operations
* `manifest` — Manifest file operations
* `config` — Configuration file operations
* `map` — Map data to a Smart Data Model
* `explorer` — Browse a Smart Data Model schema tree in the terminal
* `wizard` — Author a mapping against a schema in the terminal
* `profile` — Detect the source format of a data file
* `bugreport` — Print a diagnostic report to attach when filing a bug

###### **Options:**

* `-c`, `--config <PATH>` — Path to a configuration file
* `-v`, `--verbose` — Show the full detail of every reported problem



## `cassiopeia sdm`

Smart Data Models operations

**Usage:** `cassiopeia sdm <COMMAND>`

###### **Subcommands:**

* `download` — Download the published schema catalog into the schemas folder
* `list` — List every schema in the stored catalog
* `search` — Search the stored catalog for schemas whose name matches a query



## `cassiopeia sdm download`

Download the published schema catalog into the schemas folder

**Usage:** `cassiopeia sdm download`



## `cassiopeia sdm list`

List every schema in the stored catalog

**Usage:** `cassiopeia sdm list`



## `cassiopeia sdm search`

Search the stored catalog for schemas whose name matches a query

**Usage:** `cassiopeia sdm search <QUERY>`

###### **Arguments:**

* `<QUERY>` — The query to search for



## `cassiopeia mapping`

Mapping file operations

**Usage:** `cassiopeia mapping <COMMAND>`

###### **Subcommands:**

* `format` — Format a JSON5 mapping file
* `convert` — Convert a JSON5 mapping file to plain JSON



## `cassiopeia mapping format`

Format a JSON5 mapping file

**Usage:** `cassiopeia mapping format [OPTIONS] <MAPPING>`

###### **Arguments:**

* `<MAPPING>` — The mapping file to format

###### **Options:**

* `-r`, `--replace` — Overwrite the file with the formatted content

  Default value: `false`



## `cassiopeia mapping convert`

Convert a JSON5 mapping file to plain JSON

**Usage:** `cassiopeia mapping convert --output <OUTPUT> <MAPPING>`

###### **Arguments:**

* `<MAPPING>` — The JSON5 mapping file to convert

###### **Options:**

* `-o`, `--output <OUTPUT>` — Output path for the converted file



## `cassiopeia schema`

JSON Schema operations

**Usage:** `cassiopeia schema <COMMAND>`

###### **Subcommands:**

* `dereference` — Dereference a JSON Schema, inlining its external references



## `cassiopeia schema dereference`

Dereference a JSON Schema, inlining its external references

**Usage:** `cassiopeia schema dereference <SCHEMA>`

###### **Arguments:**

* `<SCHEMA>` — The JSON Schema file to dereference



## `cassiopeia manifest`

Manifest file operations

**Usage:** `cassiopeia manifest <COMMAND>`

###### **Subcommands:**

* `generate` — Generate a manifest file from command-line arguments



## `cassiopeia manifest generate`

Generate a manifest file from command-line arguments

**Usage:** `cassiopeia manifest generate [OPTIONS] --input <INPUT> --mapping <FILE>`

###### **Options:**

* `-i`, `--input <INPUT>` — Data source file path or URL
* `-m`, `--mapping <FILE>` — JSON5 mapping file defining the transformation rules
* `-t`, `--type <FORMAT>` — Data format of the input source

  Default value: `auto`

  Possible values:
  - `auto`:
    Detect the format from the file extension or the content
  - `csv`:
    Comma-separated values
  - `json`:
    A JSON array of objects
  - `geojson`:
    A `GeoJSON` `FeatureCollection`
  - `kml`:
    Keyhole Markup Language
  - `kmz`:
    Zipped Keyhole Markup Language
  - `grib`:
    WMO GRIB gridded binary (meteorological/climate fields)
  - `shapefile`:
    ESRI Shapefile: a geometry `.shp` with its companion `.dbf`/`.shx`/`.prj`/`.cpg` files
  - `xml`:
    Generic XML

* `--on-failure <MODE>` — How the run reacts to a failed cycle and what it exits with: abort (stop and exit non-zero), continue (run the rest, still exit non-zero), or ignore (run the rest, exit zero). Defaults to abort. Recorded on the generated manifest as `onFailure`
* `-M`, `--manifest-output <FILE>` — Output path for the generated manifest

  Default value: `manifest.json5`
* `-w`, `--writer <TYPE>` — Where to write results: the file system or a Context Broker

  Possible values:
  - `file`:
    Write entities to the file system
  - `context-broker`:
    Send entities to an NGSI-LD context broker

* `-o`, `--directory <DIRECTORY>` — Output directory (for the file writer)
* `--framing <FRAMING>` — File framing: a JSON array or line-delimited entities (file writer only)

  Default value: `array`

  Possible values:
  - `array`:
    A single pretty-printed JSON array holding every entity of the type
  - `line-delimited`:
    One compact entity per line, with no surrounding array — append-friendly and re-indent-free

* `--representation <REPRESENTATION>` — NGSI-LD representation for written entities

  Possible values:
  - `normalized`:
    Fully expanded representation carrying every attribute's type and metadata
  - `concise`:
    Compact representation omitting redundant type metadata
  - `simplified`:
    Flat key-value representation without metadata

* `--skip-null <SKIPNULL>` — How to handle null values in written entities

  Possible values:
  - `include`:
    Include null-valued attributes in output
  - `skip`:
    Omit null-valued attributes from output

* `--temporal-representation <REPRESENTATION>` — Temporal output shape. `series` folds each id's observations into one temporal entity with instance arrays (ETSI GS CIM 009 v1.9.1 clause 5.2.20); absent writes current-state (one entity per id, latest per attribute). A general output shape, valid for the file writer and the broker
* `--validation-representation <REPRESENTATION>` — NGSI-LD representation the validator serializes entities in

  Possible values:
  - `normalized`:
    Fully expanded representation carrying every attribute's type and metadata
  - `concise`:
    Compact representation omitting redundant type metadata
  - `simplified`:
    Flat key-value representation without metadata

* `--validation-skip-null <SKIPNULL>` — How the validator handles null values

  Possible values:
  - `include`:
    Include null-valued attributes in output
  - `skip`:
    Omit null-valued attributes from output

* `--validation-mode <MODE>` — How strictly schema validation is enforced, overriding the manifest: warn, fail-when-schema, or fail
* `--validation-schema <FILE|URL>` — A custom JSON Schema to validate against — a local file path or an `http(s)` URL — applied to every produced type in place of the Smart Data Models convention. A per-input `schema` in the manifest wins over this
* `-r`, `--validation-report <FILE>` — Write a JSON validation report to the given file path
* `-u`, `--broker-url <URL>` — Context Broker URL (required when --writer is context-broker)
* `--broker-operation <OPERATION>` — Which NGSI-LD operation each request performs (context-broker writer only)

  Default value: `upsert`

  Possible values:
  - `upsert`:
    Batch upsert to `entityOperations/upsert` (clause 5.6.8). The default operation
  - `create`:
    Batch create to `entityOperations/create` (clause 5.6.7)
  - `update`:
    Batch update to `entityOperations/update` (clause 5.6.9)
  - `merge`:
    Batch merge to `entityOperations/merge` (clause 5.6.20)
  - `temporal`:
    Temporal upsert to `temporal/entities` (clause 5.6.11); one entity per request, no batch form

* `--upsert-mode <MODE>` — How a batch upsert reconciles existing entities: replace them or update them in place (context-broker upsert only)

  Default value: `replace`

  Possible values:
  - `replace`:
    Replace each existing entity wholesale (clause 5.6.8 default behaviour)
  - `update`:
    Update the attributes of each existing entity in place (`?options=update`)

* `--attribute-overwrite <OVERWRITE>` — Whether a batch update overwrites existing attributes or preserves them (context-broker update only)

  Default value: `overwrite`

  Possible values:
  - `overwrite`:
    Overwrite existing attributes with the incoming values (clause 5.6.9 default behaviour)
  - `no-overwrite`:
    Preserve existing attributes and add only the missing ones (`?options=noOverwrite`)

* `--atomic` — Spool every entity and push to the broker only after a clean finish (context-broker writer only)

  Default value: `false`
* `--tenant <TENANT>` — NGSILD-Tenant header value for multi-tenant Context Brokers
* `--user-agent <USER_AGENT>` — User-Agent header the broker writer identifies itself with (context-broker writer only). Overrides the build-time default of `cassiopeia/<version>`
* `--header <NAME: VALUE>` — Extra HTTP header for Context Broker requests, written as `Name: Value`; repeat for several. Typically carries credentials, e.g. `--header "Authorization: Bearer <token>"`
* `-L`, `--link-header` — Send the `@context` via a Link header instead of embedding it in the body

  Default value: `false`
* `-C`, `--context <MODE>` — How to attach the `@context` to written entities

  Default value: `default`

  Possible values:
  - `none`:
    Do not attach any `@context`
  - `default`:
    Resolve the context URL from locally downloaded Smart Data Models
  - `url`:
    Use an explicit context URL
  - `local`:
    Inline a local `.jsonld` file's `@context` into every entity

* `--context-url <URL>` — Explicit `@context` URL (required when --context is url)
* `--context-file <FILE>` — Local `.jsonld` file to inline as the `@context` (required when --context is local)
* `--every <INTERVAL>` — Run at a fixed interval, timezone-independent (e.g. 30s, 5m, 2h)
* `--cron <EXPRESSION>` — Run on a cron schedule, evaluated in UTC (e.g. "0 0 */5 * * *").

   Six fields with seconds precision — second, minute, hour, day-of-month, month, day-of-week — plus an optional seventh year field. A five-field crontab line has no seconds field and will not parse.
* `--at <TIMES>` — Run at fixed times of day, written HH:MM and evaluated in local time (e.g. 14:00,18:00)
* `--repeat <COUNT>` — Maximum number of runs
* `--schedule-duration <DURATION>` — Stop scheduling after this duration (e.g. 2h)
* `--jitter <DURATION>` — Random jitter added to the wait between runs (e.g. 10s)
* `--retry <ATTEMPTS>` — Retry a failed run up to this many times
* `--retry-backoff <DURATION>` — Backoff between retry attempts (default 5s)



## `cassiopeia config`

Configuration file operations

**Usage:** `cassiopeia config <COMMAND>`

###### **Subcommands:**

* `generate` — Scaffold a fully-populated default configuration file at the discovered location



## `cassiopeia config generate`

Scaffold a fully-populated default configuration file at the discovered location

**Usage:** `cassiopeia config generate [OPTIONS]`

###### **Options:**

* `-o`, `--output <FILE>` — Where to write the file. Defaults to `config.toml` in the XDG config directory (or the container config root), which is where a subsequent run auto-discovers it
* `--force` — Overwrite an existing file instead of refusing to clobber it



## `cassiopeia map`

Map data to a Smart Data Model

**Usage:** `cassiopeia map [OPTIONS]`

###### **Options:**

* `-M`, `--manifest <MANIFEST>` — Path to a manifest file; it supplies the run's input, mapping, output, validation, and schedule settings, replacing the corresponding command-line options.

   A manifest describes the whole run, so it is mutually exclusive with every inline run-shaping flag — input, mapping, and type, and every output, context, validation, schedule, and failure-policy flag. Engine knobs that are not part of a run's description (the pipeline mode, the store backends, the memory profile) remain valid alongside it.
* `-i`, `--input <INPUT>` — Data source file path or URL
* `-m`, `--mapping <FILE>` — JSON5 mapping file defining the transformation rules
* `-t`, `--type <FORMAT>` — Data format of the input source

  Default value: `auto`

  Possible values:
  - `auto`:
    Detect the format from the file extension or the content
  - `csv`:
    Comma-separated values
  - `json`:
    A JSON array of objects
  - `geojson`:
    A `GeoJSON` `FeatureCollection`
  - `kml`:
    Keyhole Markup Language
  - `kmz`:
    Zipped Keyhole Markup Language
  - `grib`:
    WMO GRIB gridded binary (meteorological/climate fields)
  - `shapefile`:
    ESRI Shapefile: a geometry `.shp` with its companion `.dbf`/`.shx`/`.prj`/`.cpg` files
  - `xml`:
    Generic XML

* `--var <KEY=VALUE>` — A run-level variable, written as `KEY=VALUE`; repeat for several. Every mapping in the run can read it as `{{ vars.KEY }}`. Valid alongside `--manifest`, where it overrides a manifest global `vars` of the same name (a per-input `vars` still wins). A repeated key keeps the last value
* `--on-failure <MODE>` — How the run reacts to a failed cycle and what it exits with: abort (stop and exit non-zero), continue (run the rest, still exit non-zero), or ignore (run the rest, exit zero). Defaults to abort
* `--mode <MODE>` — Pipeline processing mode: batch (buffered parallel) or single (immediate per-item)

  Default value: `batch`

  Possible values:
  - `batch`:
    Buffered parallel processing
  - `single`:
    Immediate per-item processing

* `--validation-representation <REPRESENTATION>` — NGSI-LD representation the validator serializes entities in

  Possible values:
  - `normalized`:
    Fully expanded representation carrying every attribute's type and metadata
  - `concise`:
    Compact representation omitting redundant type metadata
  - `simplified`:
    Flat key-value representation without metadata

* `--validation-skip-null <SKIPNULL>` — How the validator handles null values

  Possible values:
  - `include`:
    Include null-valued attributes in output
  - `skip`:
    Omit null-valued attributes from output

* `--validation-mode <MODE>` — How strictly schema validation is enforced, overriding the manifest: warn, fail-when-schema, or fail
* `--validation-schema <FILE|URL>` — A custom JSON Schema to validate against — a local file path or an `http(s)` URL — applied to every produced type in place of the Smart Data Models convention. A per-input `schema` in the manifest wins over this
* `-r`, `--validation-report <FILE>` — Write a JSON validation report to the given file path
* `-w`, `--writer <WRITER>` — Where to write results: the file system or a Context Broker

  Default value: `file`

  Possible values:
  - `file`:
    Write entities to the file system
  - `context-broker`:
    Send entities to an NGSI-LD context broker

* `-o`, `--output <DIRECTORY>` — Output directory (required when --writer is file)
* `--framing <FRAMING>` — File framing: a JSON array or line-delimited entities (file writer only)

  Default value: `array`

  Possible values:
  - `array`:
    A single pretty-printed JSON array holding every entity of the type
  - `line-delimited`:
    One compact entity per line, with no surrounding array — append-friendly and re-indent-free

* `--writer-representation <REPRESENTATION>` — NGSI-LD representation for written entities

  Possible values:
  - `normalized`:
    Fully expanded representation carrying every attribute's type and metadata
  - `concise`:
    Compact representation omitting redundant type metadata
  - `simplified`:
    Flat key-value representation without metadata

* `--writer-skip-null <SKIPNULL>` — How to handle null values in written entities

  Possible values:
  - `include`:
    Include null-valued attributes in output
  - `skip`:
    Omit null-valued attributes from output

* `--temporal-representation <REPRESENTATION>` — Temporal output shape. `series` folds each id's observations into one temporal entity with instance arrays (ETSI GS CIM 009 v1.9.1 clause 5.2.20); absent writes current-state (one entity per id, latest per attribute). A general output shape, valid for the file writer and the broker
* `-u`, `--broker-url <URL>` — Context Broker URL (required when --writer is context-broker)
* `--broker-operation <OPERATION>` — Which NGSI-LD operation each request performs (context-broker writer only)

  Default value: `upsert`

  Possible values:
  - `upsert`:
    Batch upsert to `entityOperations/upsert` (clause 5.6.8). The default operation
  - `create`:
    Batch create to `entityOperations/create` (clause 5.6.7)
  - `update`:
    Batch update to `entityOperations/update` (clause 5.6.9)
  - `merge`:
    Batch merge to `entityOperations/merge` (clause 5.6.20)
  - `temporal`:
    Temporal upsert to `temporal/entities` (clause 5.6.11); one entity per request, no batch form

* `--upsert-mode <MODE>` — How a batch upsert reconciles existing entities: replace them or update them in place (context-broker upsert only)

  Default value: `replace`

  Possible values:
  - `replace`:
    Replace each existing entity wholesale (clause 5.6.8 default behaviour)
  - `update`:
    Update the attributes of each existing entity in place (`?options=update`)

* `--attribute-overwrite <OVERWRITE>` — Whether a batch update overwrites existing attributes or preserves them (context-broker update only)

  Default value: `overwrite`

  Possible values:
  - `overwrite`:
    Overwrite existing attributes with the incoming values (clause 5.6.9 default behaviour)
  - `no-overwrite`:
    Preserve existing attributes and add only the missing ones (`?options=noOverwrite`)

* `--atomic` — Spool every entity and push to the broker only after a clean finish (context-broker writer only)

  Default value: `false`
* `--tenant <TENANT>` — NGSILD-Tenant header value for multi-tenant Context Brokers
* `--user-agent <USER_AGENT>` — User-Agent header the broker writer identifies itself with (context-broker writer only). Overrides the build-time default of `cassiopeia/<version>`
* `--header <NAME: VALUE>` — Extra HTTP header for Context Broker requests, written as `Name: Value`; repeat for several. Typically carries credentials, e.g. `--header "Authorization: Bearer <token>"`
* `-L`, `--link-header` — Send the `@context` via a Link header instead of embedding it in the body

  Default value: `false`
* `-C`, `--context <MODE>` — How to attach the `@context` to written entities

  Default value: `default`

  Possible values:
  - `none`:
    Do not attach any `@context`
  - `default`:
    Resolve the context URL from locally downloaded Smart Data Models
  - `url`:
    Use an explicit context URL
  - `local`:
    Inline a local `.jsonld` file's `@context` into every entity

* `--context-url <URL>` — Explicit `@context` URL (required when --context is url)
* `--context-file <FILE>` — Local `.jsonld` file to inline as the `@context` (required when --context is local)
* `-e`, `--entity-store <STORE>` — Entity store backend for the resolver (overrides config)

  Possible values:
  - `dashmap`:
    An in-memory concurrent hash map: the fastest option, and the one that uses the most memory
  - `redb`:
    A disk-backed embedded store, for runs whose count does not fit in memory

* `--relationship-store <STORE>` — Relationship store backend for the resolver (overrides config)

  Possible values:
  - `dashmap`:
    An in-memory concurrent hash map: the fastest option, and the one that uses the most memory
  - `redb`:
    A disk-backed embedded store, for runs whose count does not fit in memory

* `--low-memory` — Use the low-memory pipeline profile for large feeds on constrained hosts
* `--batch-size <COUNT>` — How many records or entities a stage hands to the next in one batch (overrides config)
* `--channel-capacity <COUNT|none>` — How many batches may wait between two stages before the producer blocks, or `none` for unbounded handoffs (overrides config)
* `--threads <COUNT>` — How many worker threads the parallel stages share (overrides config). Defaults to the machine's performance-core count on hybrid hardware, every logical processor otherwise
* `--every <INTERVAL>` — Run at a fixed interval, timezone-independent (e.g. 30s, 5m, 2h)
* `--cron <EXPRESSION>` — Run on a cron schedule, evaluated in UTC (e.g. "0 0 */5 * * *").

   Six fields with seconds precision — second, minute, hour, day-of-month, month, day-of-week — plus an optional seventh year field. A five-field crontab line has no seconds field and will not parse.
* `--at <TIMES>` — Run at fixed times of day, written HH:MM and evaluated in local time (e.g. 14:00,18:00)
* `--repeat <COUNT>` — Maximum number of runs
* `--schedule-duration <DURATION>` — Stop scheduling after this duration (e.g. 2h)
* `--jitter <DURATION>` — Random jitter added to the wait between runs (e.g. 10s)
* `--retry <ATTEMPTS>` — Retry a failed run up to this many times
* `--retry-backoff <DURATION>` — Backoff between retry attempts (default 5s)



## `cassiopeia explorer`

Browse a Smart Data Model schema tree in the terminal

**Usage:** `cassiopeia explorer [OPTIONS]`

###### **Options:**

* `--light` — Use the light theme instead of the dark theme

  Default value: `false`



## `cassiopeia wizard`

Author a mapping against a schema in the terminal

**Usage:** `cassiopeia wizard [OPTIONS]`

###### **Options:**

* `--light` — Use the light theme instead of the dark theme

  Default value: `false`



## `cassiopeia profile`

Detect the source format of a data file

**Usage:** `cassiopeia profile <FILE>`

###### **Arguments:**

* `<FILE>` — The file to profile



## `cassiopeia bugreport`

Print a diagnostic report to attach when filing a bug

**Usage:** `cassiopeia bugreport`



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>
