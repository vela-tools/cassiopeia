# Output

Output settings decide where Cassiopeia sends finished entities and how it serializes them. The mapping defines the entity and its attributes; output chooses the destination, representation, null handling, and delivery options.

Supply output settings on the command line or in a manifest. The examples on this page use the manifest form because it keeps all output choices together.

On the command line, these settings are the `--writer`, framing, and broker flags of `cassiopeia map`. The [command-line guide](running.md#choose-the-destination) lists them.

## Choose a destination

The two destinations use different settings:

| Destination | Use it for | Main settings |
| --- | --- | --- |
| `file` | Inspecting, archiving, or handing entities to another tool | `directory`, `framing` |
| `context-broker` | Sending entities to an NGSI-LD HTTP endpoint | `url`, `operation`, `atomicity`, `contextDelivery`, `tenant`, `headers`, `userAgent` |

A minimal file output is:

~~~json5
output: {
    target: "file",
    directory: "out",
}
~~~

Cassiopeia creates the output directory when necessary and writes one file for each entity type that appears in the run. When it writes a type, it recreates that type's existing file. Use a separate or intentionally replaceable output directory when you need to preserve previous results.

## Serialize entities

The `representation` setting selects the NGSI-LD attribute representation. Cassiopeia supports the three representations defined by NGSI-LD v1.9.1:

### Normalized

Normalized output spells out every attribute type and uses the complete member names. It is the clearest representation for inspection, validation, and systems that need explicit type information:

~~~json
{
    "id": "urn:ngsi-ld:Sensor:station-17",
    "type": "Sensor",
    "temperature": {
        "type": "Property",
        "value": 21.5,
        "observedAt": "2026-08-18T13:30:00Z"
    }
}
~~~

### Concise

Concise output omits information that can be inferred from the attribute shape. A property without metadata can be written as its bare value. A property with metadata keeps an object containing `value` and the metadata. A relationship without metadata can be written as its target entity ID.

The same property with an `observedAt` qualifier becomes:

~~~json
{
    "id": "urn:ngsi-ld:Sensor:station-17",
    "type": "Sensor",
    "temperature": {
        "value": 21.5,
        "observedAt": "2026-08-18T13:30:00Z"
    }
}
~~~

Concise is useful when the consumer understands the NGSI-LD concise rules and you want to reduce repeated type metadata.

### Simplified

Simplified output keeps only the attribute value. It is the most compact form, but it does not carry metadata such as `observedAt`, `unitCode`, or `datasetId`:

~~~json
{
    "id": "urn:ngsi-ld:Sensor:station-17",
    "type": "Sensor",
    "temperature": 21.5
}
~~~

Use simplified output only when the receiving system does not need the metadata preserved by normalized and concise output.

### Select a representation

Set the representation in the output declaration:

~~~json5
output: {
    target: "file",
    directory: "out",
    representation: "normalized",
}
~~~

The writer default is `normalized`. A manifest `output.representation` or the `--writer-representation` flag on an inline run overrides that default for the run. The `representation` setting affects serialization only. It does not change the entity assembled by the mapping or the data sent to validation.

## Handle null and empty values

The `skipNull` setting accepts `skip` and `include`:

~~~json5
output: {
    target: "file",
    directory: "out",
    skipNull: "include",
}
~~~

With `skip`, Cassiopeia omits attributes whose value is null. The serializer also treats empty strings and empty language or list values as empty for the attribute kinds where that distinction applies. With `include`, those values remain in the serialized entity. The writer default is `skip`.

Null handling is separate from source resolution. A mapping can resolve a missing source field to `null`. The `skipNull` setting decides whether Cassiopeia writes the resulting attribute.

## Temporal output

Temporality is a property of attributes, not of the entity (ETSI GS CIM 009 v1.9.1 clause 4.5.5). A record marks an attribute as an observation by giving it an `observedAt`; every observation of one base ID accumulates onto that entity. The `temporal` setting chooses what is written:

| Setting | Meaning |
| --- | --- |
| absent | Current-state: one entity per ID, each attribute keeping its latest observation (ranked by `observedAt`, which is retained as a qualifier). The default. |
| `representation: "series"` | Fold every observation of one ID into a single NGSI-LD EntityTemporal (clause 5.2.20): the entity appears once and each temporal attribute is a time-ordered array of instances. |

~~~json5
output: {
    target: "file",
    directory: "out",
    temporal: {
        representation: "series",
    },
}
~~~

The setting shapes what Cassiopeia produces, not where it sends the result, so it applies to file and broker destinations alike. The series fold touches only temporal attributes. An attribute without `observedAt`, such as a name, is kept once. Cassiopeia sorts each attribute's instances oldest first, so the arrays read as a series through time.

The difference is plainest in a file. Current-state writes one object per ID holding the latest reading; `series` writes one object per ID whose attributes are instance arrays. Against a broker the stored temporal history is the same either way when observations are posted to the temporal endpoint; the representation changes only whether Cassiopeia folds each ID before delivery.

The series fold is a streaming stage between validation and writing, not a buffer over the whole run. It groups an ID's observations as they arrive and emits each folded entity as soon as the next ID begins, so it holds one ID's observations at a time rather than the run's whole output. Because a folded EntityTemporal only belongs at a broker's `/temporal/entities` endpoint, `representation: "series"` against a Context Broker requires `operation: "temporal"`.

## Write to files

File output is split by entity type. The file name combines the entity type with an extension determined by the framing and whether `@context` is embedded:

| Framing | Context in body | Extension | Contents |
| --- | --- | --- | --- |
| `array` | No | `.json` | One JSON array |
| `array` | Yes | `.jsonld` | One JSON-LD array |
| `line-delimited` | No | `.jsonl` | One JSON entity per line |
| `line-delimited` | Yes | `.ndjsonld` | One JSON-LD entity per line |

The default framing is `array`. An array file is valid, pretty-printed JSON. Empty array output is `[]`.

Line-delimited output contains one compact entity on each newline-terminated line and has no surrounding array. It suits streaming, appending, and tools that process one entity at a time. Empty line-delimited output is an empty file, not an empty JSON array.

Choose line-delimited framing when another program expects JSON Lines or when array framing is inconvenient for a large output:

~~~json5
output: {
    target: "file",
    directory: "out",
    framing: "line-delimited",
}
~~~

File output always places a resolved `@context` in the entity body. It does not use an HTTP `Link` header to carry it. If no context is resolved, the output has no `@context` and uses the non-JSON-LD extension.

## Deliver `@context`

The `context` setting controls how Cassiopeia obtains the JSON-LD context used by the output. It can be:

| Value | Meaning |
| --- | --- |
| `none` | Do not attach an `@context`. |
| `default` | Resolve the context URL from the local Smart Data Models catalog for the mapped data model. |
| A remote URL | Use that URL as the context. |
| A local path | Read `@context` from the local JSON-LD file. |

For example:

~~~json5
output: {
    target: "file",
    directory: "out",
    context: "default",
}
~~~

You can also select the context mode at input level when different inputs need different contexts. Cassiopeia resolves the effective context per entity type when a run uses multiple mappings or overrides.

## Send to a context broker

A context broker destination needs a base URL:

~~~json5
output: {
    target: "context-broker",
    url: "http://localhost:1026/",
}
~~~

Cassiopeia appends the selected NGSI-LD endpoint to this URL. The base URL should normally end in `/` so URL joining preserves its path prefix.

### Operations

The `operation` setting selects the NGSI-LD operation for each request. Each operation maps to one endpoint under ETSI GS CIM 009 v1.9.1:

| Operation | Endpoint | Request body | Use it for |
| --- | --- | --- | --- |
| `upsert` | `/ngsi-ld/v1/entityOperations/upsert` | An array of entities | The default for ordinary bulk delivery (clause 5.6.8) |
| `create` | `/ngsi-ld/v1/entityOperations/create` | An array of entities | Inserting entities that must not already exist (clause 5.6.7) |
| `update` | `/ngsi-ld/v1/entityOperations/update` | An array of entities | Updating the attributes of existing entities (clause 5.6.9) |
| `merge` | `/ngsi-ld/v1/entityOperations/merge` | An array of entities | Merging incoming attributes into existing entities (clause 5.6.20) |
| `temporal` | `/ngsi-ld/v1/temporal/entities` | One entity per request | Entities intended for the temporal API (clause 5.6.11) |

`upsert` is the default. Every entity operation posts an array and adapts the number of entities per request to the observed payload size and broker responses. `temporal` always sends one entity per request because v1.9.1 defines no batch temporal endpoint.

Two spec options refine the batch operations:

- `upsertMode` (`upsert` only) selects how Cassiopeia reconciles an existing entity. `replace` (the default) replaces it wholesale, while `update` updates its attributes in place (`?options=update`).
- `attributeOverwrite` (`update` only) selects whether Cassiopeia overwrites existing attributes. `overwrite` (the default) replaces them, while `no-overwrite` preserves attributes that are already present (`?options=noOverwrite`).

The operation does not change the mapping or convert an entity into a temporal entity. When selecting `temporal`, use a mapping that supplies the temporal fields required by the temporal API. On its own, the `temporal` operation streams each single-instance observation to the endpoint, and the broker accumulates the history. To deliver an EntityTemporal, a single entity with instance arrays from clause 5.2.20, set the temporal representation to `series` (see [Temporal output](#temporal-output)). That pairing is required for a series representation against a broker.

~~~json5
output: {
    target: "context-broker",
    url: "http://localhost:1026/",
    operation: "update",
    attributeOverwrite: "no-overwrite",
}
~~~

### Streaming and atomic delivery

The `atomicity` setting controls when Cassiopeia sends the first request:

~~~json5
output: {
    target: "context-broker",
    url: "http://localhost:1026/",
    operation: "upsert",
    atomicity: "atomic",
}
~~~

`streaming` sends entities as they reach the writer. A later pipeline failure does not retract entities the broker has already accepted.

`atomic` writes every entity to a temporary local spool first and sends nothing while the pipeline is processing. If the pipeline finishes cleanly, Cassiopeia replays the spool through the broker writer. If the pipeline fails or is cancelled, it discards the spool and sends nothing.

Atomic delivery is an all-or-nothing decision Cassiopeia makes before delivery begins. It is not a broker transaction. Once replay starts, a network failure or broker rejection can still interrupt the HTTP requests, and Cassiopeia cannot roll back requests the broker has already accepted.

The default is `streaming`, which uses less local disk and starts delivery sooner. Use `atomic` when preventing delivery from an unsuccessful pipeline matters more than temporary disk usage and delayed delivery.

### Context delivery

The `contextDelivery` setting controls how a broker request carries `@context`:

| Value | Content type | Delivery |
| --- | --- | --- |
| `body` | `application/ld+json` | Embed `@context` in every entity body |
| `link-header` | `application/json` | Omit `@context` from the body and reference a remote context in `Link` |

Body delivery is the default and works with remote, local, inline, and per-entity-type contexts. Link-header delivery requires one remote context URL. If the effective context is not a single URL, Cassiopeia warns and falls back to body delivery.

For example:

~~~json5
output: {
    target: "context-broker",
    url: "http://localhost:1026/",
    context: "https://example.org/contexts/sensor.jsonld",
    contextDelivery: "link-header",
}
~~~

The resulting `Link` header references the context with the JSON-LD relation `http://www.w3.org/ns/json-ld#context`.

### Tenant, authentication, and user agent

Use `tenant` to send the `NGSILD-Tenant` header required by a multi-tenant broker:

~~~json5
output: {
    target: "context-broker",
    url: "https://broker.example/",
    tenant: "acme",
}
~~~

Use `headers` for credentials and other broker-specific request headers. Cassiopeia forwards every configured header on every request:

~~~json5
output: {
    target: "context-broker",
    url: "https://broker.example/",
    headers: {
        Authorization: "Bearer replace-me",
        "X-Api-Key": "replace-me",
    },
}
~~~

Header values are treated as secrets by the writer and are not included in its debug representation. Keep credentials out of files that will be committed or shared. The `userAgent` field sets the HTTP `User-Agent`. When it is omitted, the build-time application default is used.

## Broker delivery behavior

The broker writer retries transport failures, HTTP 429 responses, and server-side 5xx responses with backoff. It can split a failed array request into smaller requests, allowing a batch that is too large for the broker to make progress without discarding every entity. A 413 response is treated as a size signal and causes the same split behavior.

The writer does not retry or split other client-side 4xx responses because they usually indicate a request, schema, authentication, or tenant configuration problem. It reports those entities as failed. A single entity that still fails after retry and splitting is also counted as failed.

A batch endpoint reports a partly rejected batch with `207 Multi-Status` and a `BatchOperationResult` body (ETSI GS CIM 009 v1.9.1 clause 5.2.16). The writer parses that body, counts accepted entities as written and rejected entities as failed, and reports each rejected entity's ID and problem details. A 207 is a per-entity data outcome, not a size or congestion signal, so it is neither retried nor split.

The writer checks that body against the batch it answers. If the two lists together account for fewer entities than were sent, the unaccounted entities are counted as failed and reported: their outcome is unknown, and a producer that guesses in its own favour would report data it may never have delivered. If the body cannot be read at all, the whole batch is counted as failed for the same reason.

Every non-success response is read for its RFC 7807 problem details. ETSI GS CIM 009 v1.9.1 clause 6.3.3 requires a broker to send them, and clause 5.5.3 requires enough information to act on. The run reports the broker's own `detail`. If the body is not problem details, typically because a proxy returned an error page, the run reports the status and an echo of what did arrive.

A run that reaches the broker but delivers nothing at all fails. The exit status records the failure, and the report names the most frequent reason. Partial delivery is not a failure: entities that reached the broker remain delivered, and the run summary reports the result through its counters and `Reasons` block.

Successful broker delivery means the broker accepted the entity. It does not guarantee that a later request in the same run will succeed, so inspect the run result and reported failure count for long deliveries.

## A complete destination example

This output declaration selects normalized JSON-LD entities, sends them to the broker in batch-upsert requests, and waits for the full pipeline to finish before delivering them:

~~~json5
output: {
    target: "context-broker",
    url: "https://broker.example/",
    representation: "normalized",
    skipNull: "skip",
    context: "default",
    contextDelivery: "body",
    operation: "upsert",
    atomicity: "atomic",
    tenant: "acme",
    headers: {
        Authorization: "Bearer replace-me",
    },
}
~~~

## Next steps

- [Representations](representation.md): learn how Cassiopeia chooses the representation it writes and the separate representation it uses for validation.
- [Validation](validation.md): check entities against a schema before writing them.
- [Running Cassiopeia](running.md#choose-the-destination): use these destination settings as command-line flags.
- [Manifests](manifest.md#output): see the `output` block, field by field.
- [Concepts](concepts.md) and [Write a mapping](mapping.md): learn about the entities and attributes that output serializes.
