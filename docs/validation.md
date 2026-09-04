# Validation

Before Cassiopeia writes an entity, it can check that entity against a JSON Schema for its data model. Validation runs after entity resolution and before writing, so a schema failure is caught before the data reaches a file or broker. You choose how strict the check is and which representation it uses.

Schemas are opt-in per data model. Cassiopeia checks an entity when its type has a schema on disk. In the usual mode, an entity whose type has no schema is written without a check. Smart Data Models are the usual source of these schemas. The [data-model guide](data-models.md) explains when to use one with its schema and shared vocabulary, and when to use a custom model instead.

On the command line, validation is controlled by the `--validation-*` flags of `cassiopeia map`. The [command-line guide](running.md#validate) lists them.

## Validation modes

The mode decides what a run does with each outcome. Set it with `--validation-mode` on the command line or `output.validation.mode` in a manifest. The available modes are:

| Mode | Meaning |
| --- | --- |
| `warn` | Nothing stops the run. A violation or a broken schema is reported as a warning and the entity is still written. |
| `fail-when-schema` | The default. An entity that has a schema and violates it aborts the run; a missing schema only warns and the entity is written. |
| `fail` | The strictest. Any violation, a broken schema, or a missing schema aborts the run. |

Each entity produces one of four outcomes: its schema exists and it conforms, its schema exists and it is violated, no schema is found for its type, or a schema is found but cannot be used because it is unreadable or uncompilable. The mode maps each outcome to one of three actions: Cassiopeia writes the entity, writes it while counting a warning, or aborts the run without writing it:

| Outcome | `warn` | `fail-when-schema` | `fail` |
| --- | --- | --- | --- |
| Schema exists, entity conforms | written | written | written |
| No schema found for the type | written | written, warned | run aborts |
| Schema exists, entity violates it | written, warned | run aborts | run aborts |
| Schema present but broken | written, warned | run aborts | run aborts |

The one row that separates `fail-when-schema` from `fail` is a missing schema. `fail-when-schema` lets an entity through when no schema describes its type, so a run can mix modeled and unmodeled types. A custom data model with no published schema passes unchecked while its Smart Data Model siblings are still enforced. `fail` refuses that and demands a schema for every type.

A warned entity is written. The warning is a count, not a rejection. When the mode is `warn` and no report is requested, Cassiopeia does not build the per-violation error text, which keeps a relaxed run cheap.

### What a warned run prints

Warnings are grouped by reason and entity type, not printed per entity: a stream in which every entity is nonconformant costs one line per type rather than one per entity. Each group names how many entities it stands for and one of them, so there is something concrete to open:

~~~text
! 1482 'AirQualityObserved' entities do not conform to their schema (first: urn:ngsi-ld:AirQualityObserved:ES-1234)
~~~

The three warned outcomes have their own reason codes: `schema-nonconformant`, `schema-absent`, and `schema-unusable`. Their counts appear in the run summary's `Reasons` block. The per-entity violations behind a nonconformance remain available through `--validation-report`, which provides the detail when the summary is not enough.

An aborting run reports the entity it stopped on by id, and its message names how many rules the entity broke and what the first one was. It is one line: the structured detail belongs in the report, not in the terminal.

The mode is chosen per run. A command-line `--validation-mode` overrides a manifest's `output.validation.mode`, which overrides the `fail-when-schema` default.

~~~json5
output: {
    target: "file",
    directory: "out",
    validation: {
        mode: "fail",
    },
}
~~~

Every validation option lives under one `output.validation` object: the mode above, the custom schema and representation below, plus `skipNull` and a `report` path. Each is optional and falls back to its own default when omitted.

## Validation representation

Validation checks the **simplified (key-values)** representation by default. That is the shape a Smart Data Model JSON Schema describes. The schema constrains `temperature` as the bare number `23.0`, not as the normalized Property wrapper with its `type` and `value` members. Checking the normalized form against a key-values schema would report failures caused only by structure. See the [spec-level note on why schemas validate key-values](ngsi-ld/representation.md#why-validation-uses-simplified).

The representation used for validation is separate from the representation Cassiopeia writes. Set it per run in either of two places: a command-line flag or the matching field under `output.validation`:

| Flag | Manifest field | Values | Effect |
| --- | --- | --- | --- |
| `--validation-representation` | `output.validation.representation` | `normalized`, `concise`, `simplified` | The representation entities are validated in. |
| `--validation-skip-null` | `output.validation.skipNull` | `skip`, `include` | Whether null-valued attributes are present when the entity is checked. |

A command-line flag overrides the manifest, which overrides the simplified default. Change the representation only when the target schema describes another form, such as a schema that constrains the full normalized Property wrapper with its `type` and `value` members. [Example 28](examples/28-json-advanced-schema/example.md) validates that form with `representation: "normalized"`. The [representation guide](representation.md) explains how the writing and validation settings stay independent.

## Where schemas come from

Download the published Smart Data Models catalog once:

~~~bash
cassiopeia sdm download
~~~

This populates the schemas folder. Use `cassiopeia sdm list` and `cassiopeia sdm search <query>` to inspect the catalog.

A schema is stored by its model's repository and name. A repository-qualified model such as `dataModel.OCF/Sensor` is stored at `dataModel.OCF/Sensor.json` under the schemas folder. An unqualified model such as `Sensor` is stored at `Sensor.json` in the folder root. Shared schemas referenced by models are stored there as well.

The validator uses the same convention. It reads the `dataModel` declared by the mapping. A qualified name, one containing a `/` such as `dataModel.Transportation/BikeHireDockingStation`, is looked up under its repository subdirectory. An unqualified name is looked up at the folder root. If no file is found, the type has no schema and the mode above handles it as a missing schema. This is why the mapping's `dataModel` field connects an entity to its schema.

That convention is the fallback. To validate against a schema of your own, whether one you authored for a custom model or a published schema stored elsewhere, point validation at it directly as described next.

## Custom schemas

A hand-authored schema that is not part of the downloaded catalog can be used without placing it in the schemas folder under an exact filename. Point validation at a schema source, either a local file path or an `http(s)` URL. Cassiopeia uses that source instead of the convention for the types it covers. It handles the source like `@context`, using the same distinction between local paths and web URLs and the same two run scopes. A schema is never set on the mapping.

There are two entry points, at two scopes:

| Where | Scope | How |
| --- | --- | --- |
| Per input | Every type the input's records produce | A `schema` field on a manifest input, beside its `mapping`, mirroring the per-input `context`. |
| Global | Every produced type that no per-input schema already covers | `output.validation.schema` in a manifest, or `--validation-schema` on the command line. |

The schema override is never declared in the mapping document. It lives in the manifest, just like `@context`: a per-input `schema` for the types one source produces, or a global `output.validation.schema` for the whole run. A source can be a local path or an `http(s)` URL. Cassiopeia fetches a value that parses as an `http`/`https` URL and treats anything else, such as a bare filename or a relative or absolute path, as a local file.

A per-input `schema` is resolved relative to that input's mapping, so a bare filename is read next to the mapping. A global source is resolved relative to the working directory.

~~~json5
// A per-input schema binds the types that input produces.
inputs: [
    {
        source: "planets.csv",
        mapping: "planet.json5",
        format: "csv",
        schema: "exoplanet.schema.json",
    },
],
~~~

~~~json5
// A global schema, in the output, covers every type no per-input schema names.
output: {
    target: "file",
    directory: "out",
    validation: {
        schema: "exoplanet.schema.json",
    },
}
~~~

~~~bash
# Or on the command line, for an inline run.
cassiopeia map --input planets.csv --mapping planet.json5 --type csv \
    --output out --validation-schema ./exoplanet.schema.json
~~~

### Precedence

Three levels resolve a type's schema, from most specific to least specific:

1. **A per-input `schema`**: binds every type that input produces.
2. **The global source**: `output.validation.schema` or `--validation-schema`, filling in every type no per-input schema already covers.
3. **The Smart Data Models convention**: the folder lookup above, when neither of the first two names a schema for the type.

The per-input source takes precedence over the global source, and either takes precedence over the convention. The convention remains the fallback for any type a custom source does not cover, so a mixed run can validate one model against a hand-authored schema and its siblings against the catalog.

### Local files and remote URLs

A local schema is read from disk. A remote schema is downloaded once at the start of the run into a temporary file and validated from there. The schema server is contacted once during setup, not once per entity. A remote schema that cannot be fetched, due to a connection failure or a non-success status such as `404`, aborts the run during setup before any entity is processed.

A downloaded schema is fetched as a single document. Its own relative `$ref`s are not followed and resolve to a permissive empty schema, just as an offline catalog reference does. A remote schema should therefore be self-contained or reference only published hosts whose schemas are already in the schemas folder. A local schema may `$ref` a sibling file in its own directory. Cassiopeia resolves references against the schema's own directory first and then the shared schemas folder, so a custom schema can split shared definitions into a neighbouring file while still reaching the catalog's shared schemas.

### Explicit sources are stricter than the convention

A custom source is one you expect to exist. Unlike the convention, whose missing schema is a tolerated opt-out handled by the mode, an explicitly requested schema that cannot be loaded because it is absent, unreadable, or unparseable is an error. Under `fail-when-schema` or `fail`, it aborts the run. Under `warn`, Cassiopeia reports it and writes the entity. The distinction is intentional: a convention miss means "this type has no schema, that is fine"; an explicit miss means "the schema you pointed me at is not there", which is a mistake worth surfacing.

## In an example

[Example 3](examples/03-geojson-smart-data-model/example.md) runs a strict check:

~~~bash
cassiopeia map \
    --input parking.json \
    --mapping parking.json5 \
    --type geojson \
    --output out \
    --context none \
    --validation-mode fail \
    --validation-representation simplified
~~~

`--validation-mode fail` aborts the run if any entity does not match the `OffStreetParking` schema, so a clean run proves that every entity conforms. `--validation-representation simplified` checks the key-values form described by the schema. [Example 14](examples/14-xml-unit-code/example.md) instead uses the default `fail-when-schema` in its manifest, so its `WeatherObserved` entity is enforced while any schemaless type passes through.

[Example 27](examples/27-csv-custom-schema/example.md) takes the other route: an invented `ExoPlanet` model with no catalog schema, validated against a hand-authored schema named by `--validation-schema`. It shows the custom-schema entry point in a complete run. [Example 28](examples/28-json-advanced-schema/example.md) goes further by validating the full **normalized** shape, including `unitCode`, `observedAt`, `datasetId`, sub-properties, and the less common NGSI-LD kinds, against a hand-authored schema named by a per-input `schema` in a manifest.

## Next steps

- [Running Cassiopeia](running.md): use the command line to combine a source, mapping, output, and validation in one run.
- [Representations](representation.md): learn how the writing and validation representations are chosen and why they differ.
- [NGSI-LD data representations](ngsi-ld/representation.md): see what each representation looks like and why Smart Data Model schemas validate the key-values form.
- [Choosing a data model](data-models.md): learn where a model's schema comes from.
