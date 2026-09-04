# Running Cassiopeia

`cassiopeia map` runs the pipeline. It reads a source, maps each record to an NGSI-LD entity, validates the result, and writes the entities to files or a context broker. This guide walks through that command and its sibling commands by task. It explains what each capability does, when to use it, and which flags control it. It is not an exhaustive flag reference. `cassiopeia map --help` lists every flag with its current default, and the linked topic pages cover the details.

## Two ways to run

A run is described either by inline flags or by a manifest. The two forms are mutually exclusive. Inline flags spell out the whole run on the command line:

~~~bash
cassiopeia map --input stations.csv --mapping station.json5 --output out
~~~

A manifest packages the same description, including inputs, output, validation, and schedule, into a reusable file. Run it with `--manifest` (`-M`):

~~~bash
cassiopeia map --manifest run.json5
~~~

Because a manifest describes the entire run, it conflicts with every inline run-shaping flag. Mixing `--manifest` with an inline flag such as `--output` or `--validation-mode` is a parse error, not a silent override. Engine knobs and the global `--config` are exceptions. They tune *how* the pipeline executes rather than *what* the run does, so they remain valid alongside a manifest. See [Control how the run behaves](#control-how-the-run-behaves).

Many run settings have two equivalent forms: a command-line flag and a manifest field. Examples include `--validation-mode` and `output.validation`, and `--on-failure` and the top-level `onFailure`. This guide names both forms. The manifest field reference is in the [manifest guide](manifest.md).

## Choose the source

Every run needs an input and a mapping that shapes it:

- `--input` (`-i`): the data source, a local file path or an `http`/`https` URL.
- `--mapping` (`-m`): the JSON5 mapping file that defines the transformation rules.
- `--type` (`-t`): the source format. It defaults to `auto`, which detects the format from the content, so you rarely need to set it. Give it explicitly when detection cannot tell, such as with a headerless CSV, or when you want to force a specific reader. The accepted values are `auto`, `csv`, `json`, `geojson`, `kml`, `kmz`, `grib`, `shapefile`, and `xml`.

The [source-format guide](source-formats.md) describes the records each format produces and how detection works.

## Choose the destination

`--writer` (`-w`) selects where entities go: `file` (the default) or `context-broker`.

### Write to files

A file run needs an output directory and can optionally set a framing:

- `--output` (`-o`): the output directory. Required when the writer is `file`. Cassiopeia writes one file per entity type.
- `--framing`: `array` (the default) writes one JSON array per type; `line-delimited` writes one entity per line, which suits streaming and append-oriented tools.

~~~bash
cassiopeia map -i stations.csv -m station.json5 -o out --framing line-delimited
~~~

The [output guide](output.md#write-to-files) covers the file-name and extension rules in full.

### Send to a broker

A broker run needs the broker URL and can optionally set options for each HTTP request:

- `--broker-url` (`-u`): the broker base URL. Required when the writer is `context-broker`.
- `--broker-operation`: the NGSI-LD operation each request performs. One of:

  | Value | Use it for |
  | --- | --- |
  | `upsert` | Ordinary bulk delivery. The default. |
  | `create` | Inserting entities that must not already exist. |
  | `update` | Updating attributes of existing entities. |
  | `merge` | Merging incoming attributes into existing entities. |
  | `temporal` | Posting to the temporal API, one entity per request. |

- `--upsert-mode`: for `upsert`, whether an existing entity is replaced wholesale (`replace`, the default) or updated in place (`update`).
- `--attribute-overwrite`: for `update`, whether existing attributes are overwritten (`overwrite`, the default) or preserved (`no-overwrite`).
- `--atomic`: spool every entity locally and push it to the broker only after a clean finish, so a failed run delivers nothing. Without it, entities stream to the broker as they are produced.
- `--tenant`: the `NGSILD-Tenant` header for a multi-tenant broker.
- `--user-agent`: override the default `User-Agent` the writer sends.
- `--header`: an extra request header, written `"Name: Value"`. Repeat the flag for several headers. This is where broker credentials go, for example `--header "Authorization: Bearer <token>"`.
- `--link-header` (`-L`): send `@context` in a `Link` header instead of embedding it in each entity body.

~~~bash
cassiopeia map -i readings.json -m reading.json5 \
    --writer context-broker \
    --broker-url http://localhost:1026/ \
    --broker-operation upsert \
    --header "Authorization: Bearer replace-me"
~~~

The [output guide](output.md#send-to-a-context-broker) documents the operations, [atomic delivery](output.md#streaming-and-atomic-delivery), and [context delivery](output.md#context-delivery) in full.

## Shape the entities

Three flags shape the entities themselves, independently of the destination:

- `--writer-representation`: the NGSI-LD representation for written entities: `normalized` (the default), `concise`, or `simplified`. Choose it for whatever consumes the output.
- `--writer-skip-null`: `skip` (the default) omits null-valued attributes; `include` keeps them.
- `--temporal-representation`: the temporal output shape. `series` folds each base ID's observations into a single temporal entity with time-ordered instance arrays (an EntityTemporal, ETSI GS CIM 009 v1.9.1 clause 5.2.20); absent writes current-state, with one entity per ID and each attribute keeping its latest observation. This applies to file and broker writers. It is **not** the same as `--broker-operation temporal`: this flag shapes the output, while the broker operation only selects the temporal endpoint. A `series` representation against a broker requires `--broker-operation temporal`.

The [representation guide](representation.md) explains how the written representation stays independent of the one validation checks; the [output guide](output.md#temporal-output) explains the temporal shapes.

## Attach `@context`

`--context` (`-C`) selects how Cassiopeia attaches the JSON-LD `@context`:

| Mode | Meaning |
| --- | --- |
| `none` | Attach no `@context`. |
| `default` | Resolve the context for the mapped data model from the local Smart Data Models catalog. The default. |
| `url` | Use an explicit remote URL, given with `--context-url`. |
| `local` | Inline a local `.jsonld` file, given with `--context-file`. |

`--context-url` is required when the mode is `url`, and `--context-file` is required when the mode is `local`. This mode and argument form is specific to the command line. A manifest instead carries `context` as a single string that is read by content (see the [manifest guide](manifest.md#per-input-context)). The [output guide](output.md#deliver-context) explains how the resolved context reaches a file body or a broker request.

## Validate

Validation checks each entity against its data model's JSON Schema before writing. Use these command-line controls:

- `--validation-mode`: how strict a failure is: `warn`, `fail-when-schema` (the default), or `fail`.
- `--validation-representation`: the representation used to check entities. It defaults to the key-values (simplified) form described by the schemas.
- `--validation-skip-null`: whether null-valued attributes are present when the entity is checked.
- `--validation-report` (`-r`): write a JSON validation report to the given path.

The [validation guide](validation.md) covers the modes, the outcome matrix, and where schemas come from.

## Handle failures

A cycle fails when a stage reports a fatal error, such as a validation abort under `--validation-mode fail` or a write that cannot be completed. `--on-failure` decides what the *run* does next and which exit status it returns. The same policy governs one-shot runs and repeating schedules. Its manifest equivalent is the top-level `onFailure` field. Like `--validation-mode`, the flag shapes the run and is mutually exclusive with `--manifest`.

| `--on-failure` | Scheduled run: after a failed cycle | One-shot run | Process exit |
| --- | --- | --- | --- |
| `abort` (the default) | stop the schedule immediately | the single cycle fails | non-zero |
| `continue` | log it, run the remaining cycles | finish | non-zero if **any** cycle failed |
| `ignore` | log it, run the remaining cycles | finish | **zero** always |

Warnings never fail a run. `--validation-mode warn` and the missing-schema warning under the default `fail-when-schema` mode both leave the process at exit 0 under every `--on-failure` mode.

### Exit status

Cassiopeia reports a run's outcome to the OS, so it can act as a CI gate. Exit `0` means success, with warnings allowed. A non-zero exit means a fatal abort under `abort` or `continue`. `ignore` always exits `0`. The run summary's `errors` and `warnings` counts reflect the failures used to determine the exit status.

### Read the run summary

When a run counted any errors or warnings, the summary ends with a `Reasons` block naming what they were:

~~~text
Reasons
  reason                  count  example
  broker-entity-rejected    142  attribute 'dateObserved' is not a valid DateTime
  schema-nonconformant       18  /temperature: required property missing
~~~

Each row is one reason code, how many occurrences it accounted for, and one example of it. The codes are stable, so they can be grepped for and quoted in a bug report. The rows are ordered worst first: errors before warnings, and within each, the loudest reason first.

The block is drawn only when the run actually counted something to explain. A clean run ends at the counters.

## Control how the run behaves

These settings tune execution rather than describe the run, so they remain valid alongside `--manifest`. Several also have a manifest equivalent.

**Memory profile.** `--low-memory` (equivalent to `memoryProfile: low-memory` in a manifest) selects the low-memory pipeline profile: smaller batches, sequential extraction, and disk-backed (redb) resolver stores. It trades throughput for a smaller peak footprint, allowing a large feed to run on a constrained host. It is the coarse switch. The store and mode options below tune the same trade-off more finely.

**Engine knobs.** Two more settings tune the pipeline and override configuration, with no manifest field of their own:

- `--mode`: `batch` (the default) buffers and processes items in parallel; `single` processes each item immediately as it arrives. Use `single` for the lowest latency or the simplest ordering, and `batch` for throughput.
- `--entity-store` (`-e`) and `--relationship-store`: the resolver's backing stores, `dashmap` (in-memory, the default) or `redb` (disk-backed). Use `redb` when the set of resolved entities or relationships is too large to hold in memory. `--low-memory` selects it for you.

The [manifest guide](manifest.md) is the field reference for `memoryProfile` and `onFailure`.

## Schedule a repeating run

The schedule flags (`--every`, `--cron`, `--at`, and the bounds `--repeat`, `--schedule-duration`, `--jitter`, `--retry`, `--retry-backoff`) make a run repeat instead of running once. The [scheduling guide](scheduling.md) covers them, along with cron and timezone details. `--on-failure` governs failed cycles in a schedule, as described under [Handle failures](#handle-failures).

## Global options

`--config` (`-c`) points to a configuration file. It is global, so it applies to any subcommand and remains valid alongside a manifest. Note the case: lowercase `-c` is the config file, while uppercase `-C` is the `@context` mode of a `map` run.

`--verbose` (`-v`) shows the full detail of every problem the run reports. Without it, a problem is one symbol-prefixed headline plus at most two indented lines, enough to know what went wrong and act on it. With it, the same problem lists its whole cause chain and every fact behind it: the status a broker answered with, the endpoint the request went to, the problem type it named, the entities the request carried, and the explanation it gave. It is global, so it applies to any subcommand, and it changes only how much is shown, never what the run does or what it exits with.

Repeated problems are collapsed whether or not the flag is given: an identical failure is drawn once, however many times it happens, and the run summary's `Reasons` block says how often each one fired. See [Read the run summary](#read-the-run-summary).

## Other commands

`map` is the workhorse, but the CLI carries several sibling commands for the jobs around a run:

- `cassiopeia sdm download` fetches the published Smart Data Models catalog. `cassiopeia sdm list` and `cassiopeia sdm search <query>` inspect what it holds. The catalog supplies the schemas used by validation. See the [validation guide](validation.md#where-schemas-come-from).
- `cassiopeia profile <file>` detects and reports a data file's source format without running a pipeline. It is the quickest way to see what `auto` detection will decide. See the [source-format guide](source-formats.md).
- `cassiopeia explorer` opens a terminal browser for a Smart Data Model schema, and `cassiopeia wizard` guides you through authoring a mapping against one. Both take `--light` for a light theme. See the [terminal-interface guide](tui.md).
- `cassiopeia mapping format <file>` pretty-prints a JSON5 mapping, with `--replace` to rewrite it in place. `cassiopeia mapping convert <file> --output <file>` converts a mapping to plain JSON.
- `cassiopeia schema dereference <file>` inlines a JSON Schema's external references into a single self-contained schema.
- `cassiopeia manifest generate` writes a manifest file from the same run-shaping flags accepted by `map`. You can build one interactively and then run it with `--manifest`. See the [manifest guide](manifest.md).
- `cassiopeia config generate` creates a fully populated default configuration file at the location a later run discovers automatically.

## Next steps

- [Manifests](manifest.md): package a run's inputs, output, and schedule into a reusable file.
- [Scheduling](scheduling.md): make a run repeat.
- [Command-line reference](cli.md): see every flag and subcommand with its default.
- [Examples](examples/index.md): follow complete runs you can reproduce.
