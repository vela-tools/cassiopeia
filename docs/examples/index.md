# Examples

These examples take real datasets through Cassiopeia from input to output. Each page introduces the source format, shows the record shape, includes the complete mapping, gives the command to run, and explains what the output means.

The [documentation](../index.md) explains each idea, and these examples show how to apply it to real data. Read [Concepts](../concepts.md) first if the pipeline or the difference between a mapping and a manifest is new to you. Keep the [mapping guide](../mapping.md) and the [source-format guide](../source-formats.md) nearby as you work through the examples. They are ordered so that each example builds on the previous one.

## Learning path

Start with these examples. They introduce the mapping language one idea at a time.

1. [Mapping JSON fields into entities](01-json-field-mapping/example.md): map a JSON array to `ChemicalElement` entities and type one copied field as a number.
2. [Keeping JSON entities with colliding IDs apart](02-json-id-collision/example.md): see how a weak city ID merges records, then add a geohash from the coordinates.
3. [Mapping GeoJSON to a Smart Data Model](03-geojson-smart-data-model/example.md): map GeoJSON to `OffStreetParking` and validate it against the published schema.
4. [Using CSV conditionals for nested attributes](04-csv-conditionals/example.md): read a semicolon-delimited CSV, translate codes, and build an address object.
5. [Guarding GeoJSON attributes before validation](05-geojson-attribute-guards/example.md): combine conditional values with constants and omit invalid source values.
6. [Mapping CSV files with messy headers](06-csv-messy-headers/example.md): address unusual Latin-1 CSV headers and parse comma decimals.
7. [Keeping CSV observations separate with `observedAt`](07-csv-observed-at/example.md): keep monthly measurements under one entity ID with `observedAt`.
8. [Mapping multilingual JSON with a LanguageProperty](08-json-language-property/example.md): build a `LanguageProperty` and read a hyphenated source key.
9. [Combining CSV sources with a manifest](09-csv-manifest/example.md): use a manifest to map two headerless CSV files in one run.
10. [Building JSON relationships across a hierarchy](10-json-relationships/example.md): connect region, subregion, country, and state entities.
11. [Building a CSV relationship graph across five sources](11-csv-relationship-graph/example.md): link routes, airports, airlines, aircraft models, and countries.
12. [Mapping CSV lists with a ListRelationship](12-csv-list-relationship/example.md): use a `ListRelationship` when one route has several aircraft models.
13. [Creating synthetic JSON entities from a field](13-json-synthetic-entities/example.md): create country entities from a mountain's country field.
14. [Mapping XML unit codes and observed timestamps](14-xml-unit-code/example.md): map XML measurements with `unitCode` and `observedAt`.
15. [Decoding shapefile enums into entity attributes](15-shapefile-enum-decoding/example.md): read a zipped shapefile and decode its coded columns.
16. [Mapping KMZ folder namespacing into entities](16-kmz-folder-namespacing/example.md): map placemark properties and geometry from a KMZ file.
17. [Mapping KML folder collections in one run](17-kml-folder-collections/example.md): route four KML folders to four mappings.
18. [Deriving weather values from a GRIB1 grid](18-grib1-derived-values/example.md): derive weather values from GRIB1 components.
19. [Reading GRIB2 fields by byte range](19-grib2-byte-range/example.md): fetch selected GRIB2 fields by byte range.
20. [Adding a local `@context` to CSV entities](20-csv-at-context/example.md): define a JSON-LD context for an invented model, and materialise its host star as a second entity.
21. [Mapping JSON `datasetId` instances](21-json-dataset-id/example.md): keep forecasts from several models under one attribute.
22. [Sending CSV temporal output to a broker](22-csv-broker-temporal/example.md): fold a track and deliver it to Scorpio.
23. [Scheduling repeated JSON feed mappings](23-json-scheduling/example.md): poll a sensor feed and upsert entities on a schedule.
24. [Mapping JSON arrays with a ListProperty](24-json-list-property/example.md): store hourly counts in an ordered `ListProperty`.
25. [Keeping variable GeoJSON objects in a JsonProperty](25-geojson-json-property/example.md): keep an alert's changing `parameters` object in a `JsonProperty`.
26. [Mapping CSV vocabulary terms with a VocabProperty](26-csv-vocab-property/example.md): turn an OpenStreetMap tag into a `VocabProperty` IRI.
27. [Validating two CSV custom models, each with its own schema](27-csv-custom-schema/example.md): validate two invented models against two local JSON Schemas in one run.
28. [Advanced JSON schema validation for normalized output](28-json-advanced-schema/example.md): validate wrappers and metadata in normalized output.
29. [Mapping CSV multi-attribute relationships across airport roles](29-csv-multi-attribute-relationship/example.md): give a `Flight` one `servesAirport` name for its departure and arrival airports, distinguished by `datasetId`.
30. [Mapping CSV nested relationships in movie credits](30-csv-nested-relationship/example.md): give a `Movie`'s `hasLeadActor` relationship a nested `playsCharacter` relationship and a `billingOrder` property, and a `hasCast` list relationship its own `castSize` sub-attribute.
31. [Converting GeoJSON geometry types for a GeoProperty](31-geojson-geometry-conversion/example.md): demote an administrative region's `MultiPolygon` to the largest `Polygon` and derive a map-pin centroid beside it.

## Running an example

Each example directory contains its mapping files and a download command. Clone the repository as described in [Getting started](../getting-started.md), change into an example directory, download its dataset, and run the `cassiopeia map` command. The datasets are not stored in the repository.
