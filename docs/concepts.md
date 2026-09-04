# Concepts

Cassiopeia takes records from a source and turns them into NGSI-LD entities. You can run a mapping directly from the command line or capture the same run in a manifest. A mapping describes the transformation. A manifest is an optional, reusable declaration of the inputs and run settings. This page introduces both and follows a record through the pipeline.

## Entities

Cassiopeia writes NGSI-LD entities. Each entity has an `id`, a `type`, and a set of attributes. IDs use the form `urn:ngsi-ld:<Type>:<name>`. The type comes from the data model you target.

Cassiopeia supports all NGSI-LD attribute types defined in [ETSI GS CIM 009 v1.9.1](https://www.etsi.org/deliver/etsi_gs/CIM/001_099/009/01.09.01_60/gs_cim009v010901p.pdf): `Property`, `Relationship`, `GeoProperty`, `LanguageProperty`, `VocabProperty`, `ListProperty`, `ListRelationship`, and `JsonProperty`. A `Property` holds a value, a `Relationship` points to another entity's URN, and a `GeoProperty` holds geometry. A `LanguageProperty` stores text in several languages. A `VocabProperty` stores a vocabulary IRI. `ListProperty` and `JsonProperty` hold list and JSON values, while `ListRelationship` holds a list of entity URNs. The [attribute-type reference](ngsi-ld/attribute-types.md) describes each type in detail, including its canonical payload, value member, and when to choose it over the alternatives.

The same entity can be written in three NGSI-LD representations (clause 4.5 of the [ETSI specification](https://www.etsi.org/deliver/etsi_gs/CIM/001_099/009/01.09.01_60/gs_cim009v010901p.pdf)). For a temperature reading with a timestamp, the attribute looks like this:

Normalized includes the attribute type and metadata. It is the default:

```json
"temperature": {
    "type": "Property",
    "value": 21.5,
    "observedAt": "2026-04-03T22:00:20Z"
}
```

Concise leaves out the redundant type token:

```json
"temperature": {
    "value": 21.5,
    "observedAt": "2026-04-03T22:00:20Z"
}
```

Simplified keeps only the value:

```json
"temperature": 21.5
```

All three represent the same entity. The choice affects how Cassiopeia writes the result, not how the mapping works. Cassiopeia writes normalized entities by default. Validation checks the simplified (key-values) form, which is the shape described by a Smart Data Model schema. The [representation guide](representation.md) explains how to select a representation and why validation uses its own.

## Records

A source is a sequence of records. A record might be a CSV row, an object in a JSON array, a GeoJSON feature, or a repeated element in an XML document. Cassiopeia splits the source into records and passes each one to the mapping.

Usually, one record produces one entity. A mapping can also produce a synthetic entity from the same record. This allows a source that contains only a partial reference to something else, such as an owner name in a column, to produce that related entity too.

## Mappings

A mapping tells Cassiopeia how to turn a record into an entity. It chooses the data model, defines the entity's identity, and declares its attributes. Templates read values from the record with `{{ field }}` placeholders. For example, `Station-{{ id }}` becomes `Station-7` when `id` is `7`.

Mappings are JSON5 documents. They contain the detailed rules for the transformation.

## Manifests

A manifest is an optional JSON5 document that turns a command-line run into a reusable declaration. It lists the inputs and connects each source to a mapping. It can also set the output destination, define what happens when an input fails, and schedule repeated runs.

Keeping these documents separate makes mappings reusable. A manifest packages the sources and run settings for a particular job. Use the command line for a one-off run, or put the whole job in a manifest when you want to reuse or schedule it.

## The pipeline

The pipeline handles the data in stages:

1. Collect each source.
2. Detect its format from the content, unless you declared the format.
3. Parse the source into records.
4. Apply the mapping to each record and produce attribute fragments.
5. Resolve the fragments into entities, merging fragments that belong together and adding relationships.
6. Validate each entity against a schema for its data model, when one exists. Smart Data Models are the usual source of these schemas, but they are [one option among several](data-models.md). A model you define yourself is equally valid and remains unchecked when it has no schema.
7. Write the entities to files or send them to a context broker.

Because Cassiopeia detects formats automatically, most inputs need no format setting. Validation happens before writing, so schema failures are caught before the data reaches the platform. You choose how strict validation is: the system can drop a failure, report it as a warning, or abort the run.

## Next steps

- [Source formats](source-formats.md): see the record shape each input produces before you write a mapping.
- [Write a mapping](mapping.md): turn a record into an entity.
- [Architecture](architecture.md): see how the same pipeline works internally.
