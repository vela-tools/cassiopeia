# Mapping CSV lists with a ListRelationship

This example changes one attribute in the [five-source graph](../11-csv-relationship-graph/example.md). The earlier route mapping kept only the first aircraft code because the Flight Smart Data Model defines `hasAircraftModel` as a single relationship. Routes often list several aircraft, so this version uses a `ListRelationship` and keeps every code. That matches the source better, but it no longer matches the published Flight schema.

## What changes

Only the route mapping's aircraft attribute changes. The country, aircraft-model, airport, and airline mappings, along with the other three route relationships, are unchanged.

In example 11 the attribute was a single relationship to the first code:

```json5
hasAircraftModel: {
    source: "{% if this[8] is string %}{{ this[8] | split(pat=' ') | first }}{% else %}{{ this[8] }}{% endif %}",
    type: "Relationship",
    target: {
        entity: "AircraftModel",
    },
},
```

Here it is a `ListRelationship` over the whole equipment field:

```json5
hasAircraftModel: {
    source: "{{ this[8] }}",
    type: "ListRelationship",
    target: {
        entity: "AircraftModel",
    },
},
```

## A relationship with many objects

A `Relationship` points to one entity through `object`. A `ListRelationship` points to several through `objectList`. It is the NGSI-LD type for a one-to-many link held in one attribute (ETSI GS CIM 009 v1.9.1 clause 4.5.22). A route using three aircraft types therefore has three objects in `hasAircraftModel`.

The source needs no `split`; `ListRelationship` reads it as a collection of identifiers. An array contributes one object per element, while a string is split on whitespace or commas. Thus `"744 777"` becomes links to `AircraftModel:744` and `AircraftModel:777`. A lone text code such as `CR2`, or a numeric code such as `320`, produces a one-object list.

## The catch: it is off-spec

The Flight model predates `ListRelationship`. Its schema describes `hasAircraftModel` as an array of relationship instances, not as an `objectList`, so this output does not conform. Strict validation fails with:

```text
Path '/hasAircraftModel': ["urn:ngsi-ld:AircraftModel:CR2"] is not valid under any of the schemas listed in the 'anyOf' keyword
```

That mismatch is intentional. `ListRelationship` describes the route more accurately than the published schema does. The manifest sets `validation.mode` to `"warn"`, so Cassiopeia reports the mismatch and continues. The four other schema-backed types still pass, while `Country` has no schema.

```json5
output: {
    target: "file",
    directory: "out",
    context: "none",
    validation: {
        mode: "warn",
    },
},
```

## Run it

The datasets and download commands are the same as the [previous example](../11-csv-relationship-graph/example.md).

```bash
cassiopeia map --manifest manifest.json5
```

## Read the result

A route flown with three aircraft types now carries all three links in one attribute:

```json
{
    "id": "urn:ngsi-ld:Flight:JJ-GRU-SCL",
    "type": "Flight",
    "belongsToAirline": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Airline:4867",
        "objectType": "Airline"
    },
    "departsFromAirport": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Airport:2564",
        "objectType": "Airport"
    },
    "arrivesToAirport": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Airport:2650",
        "objectType": "Airport"
    },
    "hasAircraftModel": {
        "type": "ListRelationship",
        "objectList": [
            "urn:ngsi-ld:AircraftModel:773",
            "urn:ngsi-ld:AircraftModel:320",
            "urn:ngsi-ld:AircraftModel:330"
        ],
        "objectType": "AircraftModel"
    }
}
```

The airline, departure, and arrival relationships are unchanged. Only the aircraft link has changed from one object to a list.

## Which to reach for

The [SDM-compliant version](../11-csv-relationship-graph/example.md) is the interoperable default. This version gives up that conformance to preserve all aircraft in the source, which is useful when the consumer understands `ListRelationship`. Both outputs use the same data; only the attribute's `type` changes.
