# Cassiopeia documentation

Cassiopeia turns data from common formats into [NGSI-LD](https://ngsi-ld.org/) entities. You describe how one source record maps to an entity type, and Cassiopeia handles the rest: reading, building, validating, and writing the entities to files or a context broker.

## How to read these docs

The guides below form a suggested path. Each one builds on the last: install the binary, learn what Cassiopeia produces, inspect your source, write a mapping, shape and check the output, then run and automate it. Read them in order the first time, or jump straight to the page you need. The [worked examples](examples/index.md) apply these ideas to real datasets, while the [reference pages](#reference) cover individual subjects in more depth.

## The guides

1. [Getting started](getting-started.md): install Cassiopeia from a prebuilt release binary or from source, including the optional GRIB1 system dependency.
2. [Concepts](concepts.md): learn what Cassiopeia produces, how a mapping differs from a manifest, and how a record moves through the pipeline. Read this before the how-to guides.
3. [Source formats](source-formats.md): learn the record shape Cassiopeia produces for CSV, JSON, GeoJSON, KML/KMZ, XML, Shapefiles, and GRIB, along with its format detection. This comes before mapping because mappings use the record, not the raw file.
4. [Write a mapping](mapping.md): turn a record into an entity by defining its identity, attributes, templates, transformations, nested values, relationships, metadata, and synthetic entities.
5. [Templates](templates.md): learn the Tera syntax and the Cassiopeia filters and functions available to a mapping's `source` expressions.
6. [Choosing a data model](data-models.md): learn what a data model is, when a published Smart Data Model fits, when a custom model is the better choice, and the tradeoffs of each.
7. [Output](output.md): learn about file framing, NGSI-LD representations, JSON-LD contexts, null handling, temporal output (current-state and series), and delivery to a context broker.
8. [Representations](representation.md): learn how Cassiopeia chooses the representation it writes and the separate representation it uses for validation.
9. [Validation](validation.md): learn about validation modes, the representation used for schema checks, and where schemas come from.
10. [Running Cassiopeia](running.md): use the `cassiopeia map` command line to choose a source, destination, representation, validation policy, context, failure policy, and engine profile. The guide also covers the sibling commands.
11. [Manifests](manifest.md): learn how a manifest packages inputs, output, and scheduling into a reusable run, field by field.
12. [Scheduling](scheduling.md): learn how to make a run repeat, including the cron format and the UTC-versus-local timezone rules.
13. [Examples](examples/index.md): work through real datasets end to end, with each example introducing a new mapping idea. Start with [JSON basics](examples/01-json-field-mapping/example.md).

## Reference

These pages cover individual subjects in detail. Use them from a guide or go straight to one when you need a specific reference.

- [Command-line reference](cli.md): every `cassiopeia` flag and subcommand with its default, generated from the CLI itself.
- [Running with Docker](docker.md): run the container, mount the paths it needs, and keep the catalog, inputs, and outputs between runs.
- [NGSI-LD attribute types](ngsi-ld/attribute-types.md): the eight attribute types, with each one's canonical payload, value member, defining ETSI GS CIM 009 clause, and guidance on when to use it.
- [NGSI-LD data representations](ngsi-ld/representation.md): the normalized, concise, and simplified representations at the specification level, with the clauses that define them.
- [Terminal interfaces](tui.md): the Explorer, which browses a Smart Data Model schema, and the Wizard, which guides you through authoring a mapping against a chosen model.
- [Architecture](architecture.md): how Cassiopeia works internally, including the crate layout, the staged two-phase pipeline, the intermediate representations, and the cross-cutting machinery. It is the internals companion to Concepts.
