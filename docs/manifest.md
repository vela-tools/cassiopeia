# Manifests

A manifest turns a command-line run into a reusable description. A mapping answers what one record becomes; a manifest answers which sources to read, which mapping shapes each source, where the entities go, and whether the run repeats. Anything beyond a one-off `cassiopeia map` invocation is easier to maintain in a manifest. The [command-line guide](running.md) covers the inline form and names the corresponding manifest field for each flag.

Run one with `--manifest`:

~~~bash
cassiopeia map --manifest manifest.json5
~~~

Manifests are JSON5 documents, so comments, trailing commas, and unquoted keys are allowed. The `.json5` extension is conventional. Field names use camelCase. The top level, `output`, `output.validation`, and `output.temporal` reject unknown keys, so a misspelled field produces an error instead of being silently ignored.

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
| `vars` | No | Variables every mapping in the run reads as `{{ vars.NAME }}`. See [Run variables](#run-variables). |

### Relative paths

A relative path in a manifest is resolved against the directory the manifest file is in, not the directory Cassiopeia is started from. This covers each input's `source`, `mapping`, `mappings`, and `context`, and the output's `directory`, `context`, `validation.schema`, and `validation.report`. A manifest therefore runs the same from any working directory, so `cassiopeia map --manifest run/run.json5` reads `run/stations.csv` for a `source: "stations.csv"`. A per-input `schema` is the one exception: it is resolved next to the input's mapping. Absolute paths and URLs are used as written. Paths given on the command line, including `--manifest` itself, are resolved against the working directory.

> [!WARNING]
> Migrating from v1.1.1: relative paths in a manifest used to be resolved against the working directory, except a per-input `schema`. A manifest that was always run from its own directory behaves the same. A manifest run from another directory now reads its files from the manifest's directory instead, so rewrite each of its relative paths to be relative to the manifest file, or make it absolute.

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
| `dialect` | No | What a CSV source looks like, for an input whose `format` is `csv`. See [CSV dialect](#csv-dialect). |
| `context` | No | A per-input `@context` override that wins over the output-level `context` for this input's entities. |
| `schema` | No | A per-input custom validation schema. Use a local path or an `http(s)` URL. It wins over the output-level `output.validation.schema` for the types this input produces. |
| `headers` | No | Extra HTTP headers sent on every request that fetches a remote `source`, such as an API key. See [Per-input headers](#per-input-headers). |
| `vars` | No | Variables for this input's records alone, winning over a top-level variable of the same name. See [Run variables](#run-variables). |

### One mapping or many

Each input must have exactly one of `mapping` or `mappings`. Use `mapping` for the common case of one source producing one entity type. Use `mappings` when the source divides into named collections that need different mappings, such as the folders of a KML document, the layers of a Shapefile, the tables of a GeoPackage, or the nested arrays of a JSON document. Only the collection-capable formats, `kml`, `kmz`, `shapefile`, `geopackage`, and `json`, accept `mappings`. A JSON input with `mappings` reads its nested arrays of objects as collections instead of applying the top-level record rules; see [nested collections](source-formats.md#nested-collections). A GeoPackage input with `mappings` reads only the tables it names; see [multiple tables](source-formats.md#multiple-tables).

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
| `geopackage` | OGC GeoPackage. Also spelled `geo-package`. |
| `netcdf` | NetCDF, classic or NetCDF-4. Also spelled `net-cdf`. |

The [source-format guide](source-formats.md) describes the records each format produces.

### CSV dialect

`dialect` states what a CSV source looks like, for a file whose dialect detection gets wrong or cannot see at all. Only an input that declares `format: "csv"` accepts it.

~~~json5
{
    source: "https://www.wien.gv.at/ma22-lgb/umweltgut/lumesakt-v2.csv",
    format: "csv",
    mapping: "air-quality.json5",
    dialect: {
        delimiter: ";",
        encoding: "latin1",
        skipRows: 1,
        headerRows: 3,
        decimalSeparator: ",",
        nullValues: ["NE", "---"],
    },
},
~~~

Every member is optional. A member you state wins, and Cassiopeia still detects the ones you leave out. When `delimiter`, `quote`, `skipRows`, `headerRows` (or `header: false`), `terminator`, and `encoding` are all stated, Cassiopeia detects nothing, so the run no longer depends on detection. An `escape` left out of such a dialect is `"double"`, the RFC 4180 default.

| Member | Value |
| --- | --- |
| `delimiter` | The one character that separates fields, such as `","`, `";"`, or `"\t"`. |
| `quote` | The one character that encloses a field, or `"none"` to turn quoting off. It must differ from the delimiter. |
| `escape` | How a quote inside a quoted field is written: `"double"` for a doubled quote (`""`), as RFC 4180 has it, or the one character written in front of it, such as `"\\"` for `\"`. Under an escape character, a doubled quote is still read as one quote, and the character after the escape is always taken literally, so `\\` reads as `\` and `\n` as `n`. |
| `skipRows` | The number of rows above the header to skip. Rows are counted as CSV records, so a quoted title that spans two lines is one row. A blank line is not a record and is never counted, so a title, a blank line, and the header below them is `skipRows: 1`; `skipRows: 2` would skip the header too. |
| `header` | `false` for a file without a header, whose columns are then read by position. `true` states that the file has a header and leaves its row count to detection. |
| `headerRows` | The number of rows the header spans. Each column's non-blank header cells are joined with a space, so `NO2` over `HMW` over `µg/m³` becomes the field `NO2 HMW µg/m³`. A lower header row of another width is joined over the columns it reaches. |
| `terminator` | `"lf"`, `"crlf"`, or `"cr"` (a lone carriage return, as classic Mac OS ends lines). |
| `encoding` | A label from the WHATWG Encoding Standard, such as `"utf-8"`, `"windows-1252"`, or `"iso-8859-2"`. The standard reads `"latin1"` as `windows-1252`. A label the standard maps to its `replacement` encoding, such as `"iso-2022-kr"` or `"hz-gb-2312"`, is rejected, since that encoding decodes any file to a single replacement character. A byte order mark at the start of the file wins over it. |
| `decimalSeparator` | `"."`, the default, or `","`, which reads `12,45` as the number 12.45. Under `","`, a value with a full stop, such as the date `25.10.2026`, stays a string. |
| `nullValues` | Values read as missing, such as `["NE", "---"]`, besides an empty field and `\N`. |

> [!NOTE]
> A stated `encoding` wins over a `charset` the server declares, over the source's domain, and over detection, with one exception: a byte order mark. A file that starts with a UTF-8, UTF-16LE, or UTF-16BE byte order mark is always read in the encoding the mark names, and the run reports that encoding, even when `encoding` states another. Every decoder of the WHATWG encodings reads a marked file this way, so a stated encoding cannot change how its bytes are decoded. Cassiopeia does not warn when the two disagree.

Cassiopeia rejects `header: false` together with a `headerRows` above 0, and `header: true` together with `headerRows: 0`. It also rejects a `delimiter` equal to the `quote` or to the `escape`, since the character could not both split fields and quote or escape them. A stated `delimiter` is never paired with a detected quote or escape of the same character either. It rejects an `escape` together with `quote: "none"`, since no field is quoted, and an `escape` character equal to the quote, since a quote escaped by itself is a doubled quote, stated as `"double"`. Detection never sets `decimalSeparator` or `nullValues`, because nothing in a file tells a decimal comma or a missing-value token apart from text.

`cassiopeia profile` prints the detected dialect in these member names and in manifest syntax, so a member it got wrong can be copied into `dialect` and corrected there. The [source-format guide](source-formats.md#csv) describes how each member is detected.

### Per-input context

`context` sets how this input's entities obtain their `@context`. Cassiopeia reads the value as a plain string: `none` attaches no context, `default` resolves it from the local Smart Data Models catalog, an `http`/`https` URL is used directly, and any other value is treated as a path to a local `.jsonld` file (see [local context files](output.md#local-context-files) for what it must hold). A per-input `context` overrides the output-level `context` for entities from that input.

### Per-input schema

`schema` names a custom validation schema for the types this input produces, using the same value rules as `context`. Its value can be a local file path or an `http(s)` URL. A per-input `schema` overrides the run-wide `output.validation.schema` for that input's types, and both override the Smart Data Models convention. Like `context`, the schema is never declared in the mapping document. A relative path is resolved next to the input's mapping. The [validation guide](validation.md#custom-schemas) describes the precedence in full.

### Per-input headers

`headers` adds HTTP headers to the request that fetches a remote `source`. Use it for an API key, a client identifier, or any other header an open data API needs before it answers. It has the same shape and handling as a broker output's [`headers`](output.md#send-to-a-context-broker):

~~~json5
inputs: [
    {
        source: "https://api.example/records",
        format: "json",
        mapping: "record.json5",
        headers: {
            Authorization: "Apikey replace-me",
            "X-Client-Name": "replace-me",
        },
    },
],
~~~

Cassiopeia sends every configured header on every request for that input, in every cycle of a scheduled run. The headers never go to the broker, to another input, or to a server a remote `context` or `schema` is fetched from. They do follow a redirect: when the source redirects to another host, `Authorization` is dropped, but any other header, such as `X-Api-Key`, is sent to that host too. Header values are treated as secrets and are not included in debug output. Keep them out of files that will be committed or shared by reading them from the environment, as [Environment variables](#environment-variables) describes. A local `source`, including a `file:` URL, is read without any request, so an input that pairs one with `headers` is refused. On the command line, `--input-header` sets the same headers for the `-i` input.

### Retried requests

A remote `source` that answers `429 Too Many Requests` or a `5xx` is asked again with backoff, the same way the broker writer retries the broker: up to three attempts in all, 1s and then 2s apart. When the answer carries a `Retry-After` header (RFC 9110 section 10.2.3), in seconds or as a date, the next attempt waits at least that long, even past the backoff. Only when the attempts are spent does the input fail, with the source's last answer, and with it the cycle under [`onFailure`](#top-level-structure). Any other `4xx` fails the input at once, since it points at a wrong URL, a missing key, or a refused request that asking again cannot change. A remote `schema` is fetched under the same rule. Stopping the run with CTRL+C ends a wait at once.

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
| `context` | The `@context` mode, read the same way as a per-input `context`. |
| `validation` | An object grouping the validation knobs: `mode` (`warn`, `fail-when-schema` the default, or `fail`), `schema` (a custom schema source), `representation`, and `report` (a report path). Each is optional. See [Validation](validation.md). |
| `temporal` | The temporal output shape. Set its `representation` to `"series"` for a folded EntityTemporal, or omit it for current-state output (the default). See [Output](output.md#temporal-output). |

A `file` target adds `directory` and `framing`. A `context-broker` target adds a required `url` and broker delivery settings: `operation`, `atomicity`, `contextDelivery`, `tenant`, `userAgent`, `headers`, and the operation refinements `upsertMode` and `attributeOverwrite`. The [output guide](output.md) documents every destination field, the framing and extension rules, and broker delivery in full.

## Run variables

`vars` holds values a mapping reads but the source does not, such as a network name, a dataset identifier, or a validity instant. It is an object of names to values. At the top level of the manifest it applies to every input, and on an input it applies to that input's records alone:

~~~json5
{
    version: "v1",
    vars: {
        districts: ["Center", "Bežigrad"],
    },
    inputs: [
        {
            source: "bikes.csv",
            mapping: "station.json5",
            vars: {
                network: "bikes",
            },
        },
        {
            source: "scooters.csv",
            mapping: "station.json5",
            vars: {
                network: "scooters",
            },
        },
    ],
}
~~~

A mapping reads a variable as `{{ vars.NAME }}`, so both inputs share one mapping:

~~~json5
{
    version: "v4",
    dataModel: "Station",
    identity: {
        entityName: "{{ vars.network }}-{{ id }}",
    },
    attributes: {
        name: {
            source: "{{ name }}",
        },
        network: {
            source: "{{ vars.network }}",
        },
        district: {
            source: "{{ vars.districts[0] }}",
        },
    },
}
~~~

A record with `id` 1 in `bikes.csv` becomes `urn:ngsi-ld:Station:bikes-1`, with `network` set to `bikes`. The same `id` in `scooters.csv` becomes `urn:ngsi-ld:Station:scooters-1`, so the two sources label their entities differently and never collide on an ID.

A value written in the manifest keeps its JSON type. A template can index an array, as `{{ vars.districts[0] }}` does, follow an object's members with dot access, and do arithmetic on a number. A variable is read like any source field, so [Keep a value's type](templates.md#keep-a-values-type) decides whether an attribute receives an array or a number as it is or as text. A variable that no level sets is a missing field, by the rules in [Read the record](templates.md#read-the-record).

`--var NAME=VALUE` on the command line sets a variable too, on an inline run and alongside `--manifest`. Its value is always text. A transformation such as `integer` still reads a number from it, but a template that treats it as a number, such as `{{ vars.capacity + 1 }}`, fails to render and drops its attribute.

When the same name is set in more than one place, the value is taken from the first of these that sets it:

1. The input's own `vars`.
2. `--var` on the command line. A repeated `--var` keeps its last value.
3. The top-level `vars`.

So `--var` overrides a top-level variable for the whole run, but never a variable an input sets for itself.

A mapping reads the variables beside each record, never as part of it, in the main mapping and in a [`syntheticEntity`](mapping.md#emit-a-related-entity) alike. `{{ context }}` and `this` hold only the fields the source gave, so a variable reaches an entity only through an attribute that reads it. A bare `vars` always names the variables, so a source field called `vars` is read as `{{ this['vars'] }}`, or as `this.vars.NAME` inside an expression.

`${NAME}` is ordinary text in `vars` at either level, so a variable never carries an environment value into an entity. A credential belongs in a field that reads the environment, as [Environment variables](#environment-variables) describes.

## Environment variables

A manifest can read a value from the environment as `${NAME}`, so a credential never has to be written into the file. Cassiopeia fills each one when it loads the manifest, from the environment of the process it runs in:

~~~json5
{
    version: "v1",
    inputs: [
        {
            source: "https://api.example/records?apikey=${API_KEY}",
            mapping: "record.json5",
        },
    ],
    output: {
        target: "context-broker",
        url: "https://broker.example/",
        headers: {
            Authorization: "Bearer ${BROKER_TOKEN}",
        },
    },
}
~~~

`${NAME}` works only in the fields that carry credentials: an input's `source` and `headers` values, and the output's `url` and `headers` values. Anywhere else, including [`vars`](#run-variables) and every mapping, `${NAME}` is ordinary text, so a mapping can never read an environment value into an entity. A name is letters, digits, and underscores, and does not start with a digit. Write `$${` for a literal `${`. A `$` that does not open a placeholder is ordinary text.

A variable that is not set stops the run before anything is fetched. The error names the variable and the field it sits in, such as `the output header 'authorization': the environment variable 'BROKER_TOKEN' is not set`, and never shows a value. A `source` that reads the environment must be a URL: a local path that reads a variable is refused, since a path is shown as it is wherever a message names the file.

Every value read from the environment is treated as a secret. A URL that carries one is shown with the value replaced by its placeholder, as `https://api.example/records?apikey=${API_KEY}`, in errors, diagnostics, and logs, including after a redirect and on every broker endpoint built from the output `url`. As a backstop, every log line, diagnostic, and error Cassiopeia writes has each value read replaced the same way, in the form it was read and in each percent-encoded form a URL can hold it in. A very short value can therefore mask unrelated text that happens to contain it, so prefer credentials of a realistic length.

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

## Examples

- [Example 9](https://github.com/vela-tools/cassiopeia-examples/blob/main/examples/09-csv-manifest/example.md) pairs two CSV sources with two mappings in one file-output run.
- [Example 23](https://github.com/vela-tools/cassiopeia-examples/blob/main/examples/23-json-scheduling/example.md) polls a live JSON feed every five minutes and upserts to a broker, using `schedule`, `retry`, and `onFailure`.
- [Example 17](https://github.com/vela-tools/cassiopeia-examples/blob/main/examples/17-kml-folder-collections/example.md) routes one KML source's folders to four mappings with `mappings`.

## What makes a manifest invalid

Cassiopeia rejects a manifest when `inputs` is empty, an input has both or neither of `mapping` and `mappings`, `mappings` is empty or names the same collection twice, `mappings` is used with a format that does not support collections, `dialect` is used on an input that does not declare `format: "csv"`, `headers` is used on an input with a local `source`, a `${NAME}` is malformed or names a variable that is not set, a local `source` reads an environment variable, or any field has a value outside its accepted set. Because the top level, `output`, `output.validation`, and `output.temporal` reject unknown keys, a stray or misspelled field there is also an error, and so is a setting of the other destination, such as `directory` on a `context-broker` output.

## Next steps

- [Scheduling](scheduling.md): learn about the `schedule` block, including the cron format and timezone rules.
- [Running Cassiopeia](running.md): use the inline command-line form and see how flags map to fields.
- [Output](output.md): see every `output` destination and serialization field.
- [Validation](validation.md#custom-schemas): learn about the per-input and run-wide `schema` fields.
