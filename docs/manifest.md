# Manifests

A manifest turns a command-line run into a reusable description. A mapping answers what one record becomes; a manifest answers which sources to read, which mapping shapes each source, where the entities go, and whether the run repeats. Anything beyond a one-off `cassiopeia map` invocation is easier to maintain in a manifest. The [command-line guide](running.md) covers the inline form and names the corresponding manifest field for each flag.

Run one with `--manifest`:

~~~bash
cassiopeia map --manifest manifest.json5
~~~

Manifests are JSON5 documents, so comments, trailing commas, and unquoted keys are allowed. The `.json5` extension is conventional. Field names use camelCase. The top level rejects unknown keys, so a misspelled field produces an error instead of being silently ignored.

## Top-level structure

~~~json5
{
    version: "v1",
    inputs: [
        {
            source: "stations.csv",
            mapping: "station.json5",
            format: "csv",
        },
    ],
    output: {
        target: "file",
        directory: "out",
    },
}
~~~

| Field | Required | Value |
| --- | --- | --- |
| `version` | Yes | The manifest schema version. The only accepted value is `"v1"`. |
| `inputs` | Yes | A non-empty array of inputs. See [Inputs](#inputs). |
| `output` | No | Where and how entities are written. Application defaults apply when omitted. See [Output](#output). |
| `onFailure` | No | How the run reacts to a failed cycle and what it exits with: `abort` (the default) stops and exits non-zero, `continue` runs the rest and exits non-zero, `ignore` runs the rest and exits zero. Governs one-shot and scheduled runs alike. See [Handle failures](running.md#handle-failures). |
| `schedule` | No | Repeat the run on a schedule instead of running once. See [Schedule](#schedule). |
| `memoryProfile` | No | `default` (the default) or `low-memory`, which trades throughput for a smaller footprint. |

## Inputs

`inputs` is an array of one or more entries, each pairing a source with the mapping that shapes it. Cassiopeia processes multiple inputs independently, then merges their entities at the resolver. One manifest can therefore produce several entity types from the same source or from different sources.

~~~json5
inputs: [
    {
        source: "airports.dat",
        mapping: "airport.json5",
        format: "csv",
    },
    {
        source: "airlines.dat",
        mapping: "airline.json5",
        format: "csv",
    },
],
~~~

| Field | Required | Value |
| --- | --- | --- |
| `source` | Yes | A local file path or an `http`/`https` URL. A `file:` URL is read as a local path. |
| `mapping` | One of two | The path to a single mapping file. |
| `mappings` | One of two | A list of per-collection mappings, for a source split into named collections. |
| `format` | No | The source format. Detected from the content when omitted. |
| `context` | No | A per-input `@context` override that wins over the output-level `context` for this input's entities. |
| `schema` | No | A per-input custom validation schema. Use a local path or an `http(s)` URL. It wins over the output-level `output.validation.schema` for the types this input produces. |

### One mapping or many

Each input must have exactly one of `mapping` or `mappings`. Use `mapping` for the common case of one source producing one entity type. Use `mappings` when the source divides into named collections that need different mappings, such as the folders of a KML document or the layers of a Shapefile. Only the collection-capable formats, `kml`, `kmz`, and `shapefile`, accept `mappings`.

A `mappings` list contains objects that bind collection names to mappings. Cassiopeia matches each collection name verbatim and leaves any collection that is not listed unmapped:

~~~json5
{
    source: "nyc-cycling.kml",
    format: "kml",
    mappings: [
        {
            collection: "Citi Bike Stations",
            mapping: "station.json5",
        },
        {
            collection: "Where to Get Bikes",
            mapping: "rental.json5",
        },
    ],
},
~~~

### Formats

The `format` field accepts one of the following values. When omitted, it defaults to `auto`, which detects the format from the source content.

| Value | Format |
| --- | --- |
| `auto` | Detect from the content. The default. |
| `csv` | Comma-separated values. |
| `json` | JSON. |
| `geojson` | GeoJSON. Also spelled `geo-json`. |
| `kml`, `kmz` | KML, and zipped KML. |
| `xml` | Generic XML. |
| `grib` | GRIB gridded binary. |
| `shapefile` | ESRI Shapefile, plain or zipped. |

The [source-format guide](source-formats.md) describes the records each format produces.

### Per-input context

`context` sets how this input's entities obtain their `@context`. Cassiopeia reads the value as a plain string: `none` attaches no context, `default` resolves it from the local Smart Data Models catalog, an `http`/`https` URL is used directly, and any other value is treated as a path to a local `.jsonld` file. A per-input `context` overrides the output-level `context` for entities from that input.

### Per-input schema

`schema` names a custom validation schema for the types this input produces, using the same value rules as `context`. Its value can be a local file path or an `http(s)` URL. A per-input `schema` overrides the run-wide `output.validation.schema` for that input's types, and both override the Smart Data Models convention. Like `context`, the schema is never declared in the mapping document. A relative path is resolved next to the input's mapping. The [validation guide](validation.md#custom-schemas) describes the precedence in full.

## Output

The `output` block selects a destination and controls how entities are serialized. `target` is required and picks the destination. The remaining fields are shared across destinations.

~~~json5
output: {
    target: "context-broker",
    url: "http://localhost:1026/",
    operation: "upsert",
    context: "default",
    representation: "normalized",
    validation: {
        mode: "fail-when-schema",
    },
},
~~~

| Field | Value |
| --- | --- |
| `target` | `file` or `context-broker`. Required. |
| `representation` | `normalized` (the default), `concise`, or `simplified`. See [Representations](representation.md). |
| `skipNull` | `skip` (the default) or `include`. |
| `context` | The `@context` mode, read the same way as a per-input `context`. |
| `validation` | An object grouping the validation knobs: `mode` (`warn`, `fail-when-schema` the default, or `fail`), `schema` (a custom schema source), `representation`, `skipNull`, and `report` (a report path). Each is optional. See [Validation](validation.md). |
| `temporal` | The temporal output shape. Set its `representation` to `"series"` for a folded EntityTemporal, or omit it for current-state output (the default). See [Output](output.md#temporal-output). |

A `file` target adds `directory` and `framing`. A `context-broker` target adds a required `url` and broker delivery settings: `operation`, `atomicity`, `contextDelivery`, `tenant`, `userAgent`, `headers`, and the operation refinements `upsertMode` and `attributeOverwrite`. The [output guide](output.md) documents every destination field, the framing and extension rules, and broker delivery in full.

## Schedule

A `schedule` makes the run repeat instead of running once and exiting. `mode` and `value` set the trigger. The remaining fields limit and control the repetition. The [scheduling guide](scheduling.md) covers the triggers in depth, including the cron seconds-precision format and the timezone rules. This section summarizes the fields.

~~~json5
schedule: {
    mode: "every",
    value: "5m",
    jitter: "20s",
    retry: {
        maxAttempts: 3,
        backoff: "15s",
    },
},
~~~

| Field | Required | Value |
| --- | --- | --- |
| `mode` | Yes | The trigger type: `every`, `cron`, or `at`. |
| `value` | Yes | The trigger value, matched to `mode` (below). |
| `repeat` | No | The maximum number of runs. At least 1. Unbounded when omitted. |
| `duration` | No | The maximum total running time, such as `"24h"`. Stops after this regardless of `repeat`. |
| `jitter` | No | A random extra delay before each run, such as `"10s"`, to spread load when many deployments start together. |
| `retry` | No | A retry policy for a run that fails. |

The top-level [`onFailure`](#top-level-structure) field controls how a failed cycle is handled, including whether it stops the schedule and what exit status the run returns. It is not a `schedule` field, and its default is `abort`.

The trigger value depends on the mode:

| Mode | `value` | Example |
| --- | --- | --- |
| `every` | An interval | `"30s"`, `"5m"`, `"2h"` |
| `cron` | A six-field cron expression, evaluated in UTC | `"0 0 9 * * *"` |
| `at` | An array of daily local times, `HH:MM` | For example, `06:00` and `18:00` |

`duration`, `jitter`, and an interval `value` are duration strings. The `retry` policy requires a `maxAttempts` of at least 1 and accepts an optional `backoff` duration between attempts. The default is `5s`:

~~~json5
retry: {
    maxAttempts: 3,
    backoff: "5s",
},
~~~

## Examples in the tree

- [Example 9](examples/09-csv-manifest/example.md) pairs two CSV sources with two mappings in one file-output run.
- [Example 23](examples/23-json-scheduling/example.md) polls a live JSON feed every five minutes and upserts to a broker, using `schedule`, `retry`, and `onFailure`.
- [Example 17](examples/17-kml-folder-collections/example.md) routes one KML source's folders to four mappings with `mappings`.

## What makes a manifest invalid

Cassiopeia rejects a manifest when `inputs` is empty, an input has both or neither of `mapping` and `mappings`, `mappings` is empty or names the same collection twice, `mappings` is used with a format that does not support collections, or any field has a value outside its accepted set. Because the top level rejects unknown keys, a stray or misspelled field is also an error.

## Next steps

- [Scheduling](scheduling.md): learn about the `schedule` block, including the cron format and timezone rules.
- [Running Cassiopeia](running.md): use the inline command-line form and see how flags map to fields.
- [Output](output.md): see every `output` destination and serialization field.
- [Validation](validation.md#custom-schemas): learn about the per-input and run-wide `schema` fields.
