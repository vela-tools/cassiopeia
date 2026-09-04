# Architecture

Cassiopeia converts external data into NGSI-LD entities. It reads files or remote sources, applies mappings, validates the result, then writes entities to files or sends them to a Context Broker. It produces context data, but it does not store or broker that data.

This document describes the system's shape and the decisions that hold it together. [Concepts](concepts.md) introduces the same process from a user's point of view.

## Design goals

Cassiopeia is organized around a few architectural goals:

- Keep source formats, mappings, the NGSI-LD model, validation, and output independent.
- Use the type system to make domain mistakes difficult to express.
- Let each stage do one job and communicate through an explicit boundary.
- Process large inputs with bounded memory and parallel work where it is safe.
- Make failures, cancellation, progress, and resource use visible to the rest of the system.

These goals explain why the pipeline is staged, why it has a shared store in the middle, and why the command-line tools do not contain transformation logic.

## System shape

The data path is:

collect -> profile -> ingest -> expand -> resolve -> extract -> transform -> validate -> write

The first four stages prepare and interpret source data. The middle of the pipeline combines information that belongs to the same entity. The remaining stages construct, check, and emit complete NGSI-LD entities.

The pipeline is divided into two phases. During the first phase, every input has its own processing lane. Each lane collects its source, identifies the format, decodes records, and expands those records into partial entity contributions. The lanes then feed a shared resolver.

The resolver stores and combines those contributions. When all input lanes have finished, the second phase reads the assembled entities and sends them through extraction, transformation, validation, and writing. This boundary is necessary because an entity can receive data from several records, inputs, or mappings.

The scheduler sits beside this data path. It decides when a cycle starts and whether another cycle should follow. A one-shot job and a scheduled job use the same pipeline.

## The stages

| Stage | Responsibility |
|---|---|
| Collection | Make each configured source available locally, downloading remote inputs when needed. |
| Profiling | Determine the source format and record the details needed to decode it. |
| Ingestion | Decode the source into records while preserving source values and collection information. |
| Expansion | Apply a mapping and turn one record into one or more partial entity contributions. |
| Resolution | Combine contributions, resolve identity and relationships, and hold the assembled data. |
| Extraction | Evaluate mapping expressions against assembled source data. |
| Transformation | Express the resolved values as NGSI-LD entities and attributes. |
| Validation | Check entities against the applicable JSON Schemas and apply the run's validation policy. |
| Writing | Deliver entities to files or a Context Broker. |

### Collection and profiling

The collector accepts local paths and remote locations. A local source can be read directly. A remote source is fetched before the format-specific work begins, so the rest of the pipeline can treat both cases in the same way.

The profiler determines what a source contains. Detection is based on content and checks specific formats before general ones. That ordering matters when formats overlap, such as KML and XML or structured JSON and delimited text. A caller can provide a format hint, but the profiler still inspects the source details needed by the chosen ingestor. The [source formats guide](source-formats.md) documents the supported formats and their record shapes.

### Ingestion

Ingestors know about source formats, not about the target data model. They decode supported formats such as CSV, JSON, GeoJSON, KML and KMZ, XML, Shapefile, and GRIB into records. A record keeps the source fields and, for formats with folders or layers, the collection label that can select a mapping later.

Each format has its own record boundary. CSV usually produces one record per row, GeoJSON one per feature, and KML one per placemark. JSON and XML use deterministic rules to find arrays of records. Shapefile layers and KML folders can be treated as separate collections within one source. These format-specific rules are described in the [source formats guide](source-formats.md).

Coordinate conversion is part of input preparation. Source coordinates are reprojected to EPSG:4326 before the mapping consumes them.

### Expansion

Following ingestion, the data enters the Expander for structural expansion. A source record often contains data for more than one logical entity, so one record can produce several partial contributions. For example, a row describing a transit station may produce a station contribution and contributions for associated equipment.

The Expander applies the selected mapping, derives entity identities, and creates relationship references. A mapping can also ask it to materialize a related synthetic entity from the same record. The [mapping guide](mapping.md) covers those mapping features in detail. The Expander creates the pieces that the resolver will combine; it does not build the final NGSI-LD document.

### Resolution

The resolver is the stateful centre of the pipeline. It groups contributions by entity identity, merges information from different inputs, and keeps relationship information until the input phase is complete. This is where duplicate contributions become one entity, and where an entity's repeated observations over time accumulate onto its attributes rather than fanning out into separate entities. Whether the store keeps only the latest observation per attribute or the whole series is chosen from the run's temporal output setting.

The store can be memory backed for normal workloads or disk backed for datasets that exceed the available memory budget. Storage is a runtime choice controlled by configuration. It does not change the mapping or the shape of the final entity.

### Assembly, extraction, and transformation

Once the input phase is complete, the assembler scans the resolver's store and combines each entity id's stored contributions, scope, and relationships into an assembled entity. This is the resolver's output side, and it runs as its own stage: a current-state store yields one assembled entity per id, while a series store yields one per observation. The extractor then evaluates the mapping against each assembled entity's source data, resolving attribute values, metadata, nested attributes, language maps, and relationships. The transformer expresses those resolved values in the NGSI-LD domain model.

Keeping these responsibilities separate makes the boundary clear. Assembly answers "which stored contributions make up this entity?" Extraction answers "what value does this mapping produce?" Transformation answers "how is that value represented in NGSI-LD?" A failed or non-finite calculation produces an absent attribute rather than invalidating the whole source record.

### Validation

The validator checks an entity against the schema for its data model when one is available. It distinguishes three cases: no schema was found, the entity conforms to the schema, or the entity violates it. The run policy decides whether a missing schema or a violation is ignored, reported as a warning, or treated as a failure. The [validation guide](validation.md) documents the modes, schema lookup, and representation used for validation.

Schemas can come from the Smart Data Models catalog or from a user-managed schema directory. Qualified model names identify the catalog repository used for lookup, while an explicit schema can override the normal location. References are resolved against local schema files so validation does not require network access during a run.

### Writing

The writer is the final stage. File output can use a JSON array or one entity per line. Broker output sends batches through the corresponding NGSI-LD operations for creating, updating, merging, or upserting entities. The [output guide](output.md) documents destinations, framing, broker delivery, and context handling.

Writing can be preceded and decorated by additional policies. Atomic output is a writer decorator held back until a cycle succeeds, which prevents a failed or cancelled cycle from publishing a partial result. A series temporal output inserts a separate aggregation stage between validation and writing: it folds each entity's single-instance observations into one NGSI-LD EntityTemporal (ETSI GS CIM 009 v1.9.1 clause 5.2.20), holding one entity at a time. A current-state run omits that stage.

## Why the pipeline has two phases

The two phases are a consequence of entity assembly, not an arbitrary division of the stages. The input side works with source records and does not know whether another input will contribute to the same entity. The output side needs a complete view before it can construct and validate that entity.

The first phase is therefore a fan-in: many input lanes contribute to one shared store. The second phase is a fan-out: the store releases assembled entities to the output stages. The resolver is the seam between them, and it is the only component that needs to retain the complete intermediate state of a cycle.

This arrangement also isolates source-specific parallelism from entity-specific work. Input lanes can run independently, while assembly remains centralized and consistent.

## Execution model

The pipeline uses worker threads for the processing stages and channels for communication between them. Each input lane can progress independently, and a bounded channel slows a producer when its consumer falls behind. This back-pressure keeps the amount of in-flight data within the configured memory budget. An unbounded channel is available where allowing a producer to run ahead is safe.

The CPU-heavy stages process batches and use parallel iterators when records can be handled independently. Work that depends on order or shared state keeps a sequential boundary. Read-only mappings and configuration are shared between workers instead of being rebuilt for every record.

I/O-bound work is concurrent at the edges. Schema catalog downloads use asynchronous work with limits and retries. Output can use several workers for broker requests, while a bounded queue prevents network latency from causing unbounded memory growth.

Batch size, channel capacity, extraction parallelism, and storage backend are runtime settings. The low-memory profile combines smaller batches, tighter channel bounds, disk-backed stores, and reduced extraction parallelism.

Cancellation is shared across the stages in a cycle. Workers check it between units of work and finish at a stage boundary. Atomic output is committed only after successful completion, so cancellation does not publish an incomplete result.

## Strong typing and intermediate data

Cassiopeia uses the type system to keep concepts separate across stage boundaries. Entity identifiers, IRIs, names, source formats, timestamps, language tags, and coordinate systems are different domain values rather than interchangeable strings. A stage therefore receives data that describes what it may do, instead of a collection of unstructured values.

The pipeline also has a deliberate progression of data shapes. Ingestion produces source records. Expansion adds the mapping context, target identity, and relationship information needed to create partial entity contributions. Resolution combines those contributions. Extraction and transformation produce the final NGSI-LD entity. Each boundary narrows the gap between source data and the output standard.

Failures follow the same rule. Stages report typed errors with their source context intact. A stage failure reaches the orchestrator and the run report instead of being swallowed, and a worker panic is converted into a recorded pipeline failure.

## The NGSI-LD domain

The NGSI-LD domain model is independent of ingestion, mappings, storage, and the command line. It defines what Cassiopeia is allowed to emit under NGSI-LD v1.9.1: an entity has a context, an identifier, a type, an optional scope, and a set of attributes.

The attribute model covers properties, relationships, geo-properties, language properties, vocabulary values, lists of values, lists of relationships, and JSON properties. It also covers multiple instances of an attribute through dataset identifiers and standard metadata such as observation time and unit codes.

Geometry is its own domain type rather than a shape of general JSON. It is restricted to the six geometry types clause 4.7 admits, which makes a `GeometryCollection` in a GeoProperty unrepresentable rather than merely invalid, and rings are closed and rewound to RFC 7946's right-hand rule on the way in. A separate crate owns the conversion lattice between those six types, so the same rules govern an attribute's `geometry` block and the geometry template functions.

Entities can be serialized in normalized, concise, or simplified form as defined by [ETSI GS CIM 009 v1.9.1](https://www.etsi.org/deliver/etsi_gs/CIM/001_099/009/01.09.01_60/gs_cim009v010901p.pdf). The representation choice belongs to the boundary where the entity is consumed. Normalized output is the default for writing, while schema validation can use the simplified shape expected by Smart Data Model schemas. The [NGSI-LD attribute reference](ngsi-ld/attribute-types.md) and [representation guide](ngsi-ld/representation.md) cover the domain details.

Qualified and unqualified data models are also distinct. A qualified model name disambiguates models with the same local name and gives schema lookup the repository information it needs. The [data model guide](data-models.md) explains how published and custom models fit into this boundary.

## Mappings and templates

A mapping is a reusable description of how source data becomes an entity. It defines the data model, identity, attributes, relationships, metadata, and any nested or synthetic entities. Mappings do not know where a job runs or where its output goes. The [mapping guide](mapping.md) explains the document and transformation rules, while the [templates reference](templates.md) covers the expression language and built-in helpers.

The mapping engine treats common expressions as direct work. Static values, field lookups, and simple concatenation avoid the full template evaluator. Expressions that need functions, filters, or control flow use the evaluator. This keeps ordinary mappings inexpensive without limiting more complex transformations. The implementation detail matters here because mappings are evaluated for every source record; usage examples belong in the [templates reference](templates.md).

Mappings can clean values, perform calculations, work with time, and construct geometry. A failed or non-finite calculation removes the affected attribute and allows the rest of the record to continue.

## Manifests, configuration, and scheduling

A manifest describes a job. It lists the inputs, chooses their mappings, selects the destination, and can set validation, failure, scheduling, and memory policies. Separating the manifest from the mapping makes the transformation reusable across sources and destinations. See the [manifest guide](manifest.md) for the document structure and field definitions.

Configuration describes the environment in which the job runs. It supplies paths for mappings and schemas, source and output settings, channel and batch choices, and resolver storage settings. Defaults, the configuration file, environment variables, and command-line overrides are merged in a fixed order. The memory policy is applied last so it can enforce the final resource choice regardless of where it was configured. The [running guide](running.md) describes the command-line settings, and the [manifest guide](manifest.md) describes the run-level settings.

The scheduler can run a manifest once or repeat it on an interval, cron schedule, or set of fixed times. Schedules may include limits, jitter, retries, and backoff. Failure policy determines whether a failed cycle stops the job, is reported while later cycles continue, or is ignored. The [scheduling guide](scheduling.md) covers the available triggers and retry behaviour.

## Schemas, reporting, and tools

The Smart Data Models catalog is a schema source, not the definition of the NGSI-LD domain. Cassiopeia downloads and stores catalog schemas, rewrites their published references to local paths, and can provide a flattened view for inspection. A user-defined data model is still valid when no catalog schema exists for it. The [data model guide](data-models.md) explains the choice between published and custom models, and the [validation guide](validation.md#where-schemas-come-from) explains how the stored schemas are used.

Reporting is separate from processing. A run can show interactive stage progress, emit structured telemetry, or remain quiet without changing the pipeline. The same measurements feed all of those modes: stage counts and timings, channel pressure, warnings, errors, and run totals.

The CLI is the composition point that loads configuration and manifests, creates the pipeline, and reports the result. The terminal tools are separate consumers of the surrounding data: one helps inspect schemas and the other helps author mappings. The [terminal interfaces guide](tui.md) documents those tools. Neither belongs inside the runtime data path.
