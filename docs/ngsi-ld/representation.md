# NGSI-LD data representations

NGSI-LD can serialize the same entity in three forms. Normalized and concise representations keep the information needed to reconstruct its attributes. Simplified representation keeps the values, which makes it easier to consume, but drops type information and attribute metadata. This page compares the three forms using one entity, then explains the special case of relationship arrays. The [representation guide](../representation.md) explains how Cassiopeia selects a form, and the [attribute-type reference](attribute-types.md) describes the attribute types.

The examples use the member names from ETSI GS CIM 009 v1.9.1: `vocab` for `VocabProperty`, `valueList` for `ListProperty`, and `json` for `JsonProperty`.

## One entity, three shapes

The entity below comes from [example 14](../examples/14-xml-unit-code/example.md). It contains one station reading. The `temperature` attribute includes both `unitCode` and `observedAt`, so the three representations show a useful difference in how much information they keep.

### Normalized

Normalized is the complete attribute form. Each attribute is an object with a `type` member and a type-specific value member. Qualifiers such as `observedAt` and `unitCode` stay next to that value. ETSI GS CIM 009 v1.9.1 defines normalized Property and Relationship representations in clauses 4.5.2 and 4.5.3.

```json
{
    "id": "urn:ngsi-ld:WeatherObserved:65142878",
    "type": "WeatherObserved",
    "stationName": {
        "type": "Property",
        "value": "LJUBLJANA/BEZIGRAD"
    },
    "dateObserved": {
        "type": "Property",
        "value": "2026-08-21T10:00:00.000Z"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                14.5172,
                46.0658
            ]
        }
    },
    "temperature": {
        "type": "Property",
        "value": 23.0,
        "observedAt": "2026-08-21T10:00:00Z",
        "unitCode": "CEL"
    },
    "relativeHumidity": {
        "type": "Property",
        "value": 0.8
    },
    "atmosphericPressure": {
        "type": "Property",
        "value": 1011,
        "unitCode": "A97"
    },
    "windSpeed": {
        "type": "Property",
        "value": 0.2,
        "unitCode": "MTS"
    },
    "windDirection": {
        "type": "Property",
        "value": 54,
        "unitCode": "DD"
    },
    "visibility": {
        "type": "Property",
        "value": 20,
        "unitCode": "KMT"
    }
}
```

For file output, Cassiopeia writes normalized entities by default. The form is verbose, but it leaves the type and metadata visible to a consumer.

### Concise

Concise is also lossless. It removes a redundant `type` member when the remaining shape makes the attribute type clear. A Property with no metadata can therefore be a bare value. A Property with qualifiers remains an object, but its `type` is omitted. A GeoProperty without sub-attributes can be recognized from its GeoJSON shape. ETSI describes concise Property and Relationship serialization alongside normalized serialization in clauses 4.5.2 and 4.5.3.

```json
{
    "id": "urn:ngsi-ld:WeatherObserved:65142878",
    "type": "WeatherObserved",
    "stationName": "LJUBLJANA/BEZIGRAD",
    "dateObserved": "2026-08-21T10:00:00.000Z",
    "location": {
        "type": "Point",
        "coordinates": [
            14.5172,
            46.0658
        ]
    },
    "temperature": {
        "value": 23.0,
        "observedAt": "2026-08-21T10:00:00Z",
        "unitCode": "CEL"
    },
    "relativeHumidity": 0.8,
    "atmosphericPressure": {
        "value": 1011,
        "unitCode": "A97"
    },
    "windSpeed": {
        "value": 0.2,
        "unitCode": "MTS"
    },
    "windDirection": {
        "value": 54,
        "unitCode": "DD"
    },
    "visibility": {
        "value": 20,
        "unitCode": "KMT"
    }
}
```

### Simplified

Simplified representation, often called key-values, keeps each attribute's value and removes the surrounding attribute object. A consumer requests it with `options=keyValues`. ETSI defines it in clause 4.5.4. A Property becomes its value, a Relationship becomes its target URI or URIs, and a GeoProperty becomes its GeoJSON geometry.

```json
{
    "id": "urn:ngsi-ld:WeatherObserved:65142878",
    "type": "WeatherObserved",
    "stationName": "LJUBLJANA/BEZIGRAD",
    "dateObserved": "2026-08-21T10:00:00.000Z",
    "location": {
        "type": "Point",
        "coordinates": [
            14.5172,
            46.0658
        ]
    },
    "temperature": 23.0,
    "relativeHumidity": 0.8,
    "atmosphericPressure": 1011,
    "windSpeed": 0.2,
    "windDirection": 54,
    "visibility": 20
}
```

The `temperature` attribute makes the tradeoff easy to see. In normalized form, it keeps its type, value, observation time, and unit:

```json
{
    "temperature": {
        "type": "Property",
        "value": 23.0,
        "observedAt": "2026-08-21T10:00:00Z",
        "unitCode": "CEL"
    }
}
```

In concise form, the type is omitted because the `value` member identifies a Property:

```json
{
    "temperature": {
        "value": 23.0,
        "observedAt": "2026-08-21T10:00:00Z",
        "unitCode": "CEL"
    }
}
```

In simplified form, only the number remains:

```json
{
    "temperature": 23.0
}
```

The receiving system must already know that this number is in degrees Celsius and when it was observed, because simplified representation does not carry that metadata.

## Relationships and arrays

Relationship arrays can mean different things, so the representation matters. A normalized Relationship instance uses an `object` member. ETSI allows that member to contain one URI or an array of URIs. If the same relationship name has several separate instances, the entity-level attribute is an array of Relationship objects, and `datasetId` distinguishes the instances when needed.

That is different from a `ListRelationship`. A normalized ListRelationship uses `objectList` for one ordered collection of target URIs. Its simplified form is a bare ordered array of URIs. A ListRelationship can also have multiple dataset-specific instances under the multi-attribute rules in clause 4.5.5.

A bare array of normalized Relationship objects is not a substitute for a single Relationship object. It represents multiple instances of the same attribute, so each instance needs its own identity through `datasetId`, except that one instance may be the default and omit it. Do not copy a simplified relationship array directly into normalized output.

Cassiopeia's mapping must therefore choose the construct that matches the source. Use a `ListRelationship` for one ordered collection of links. Use multiple Relationship or ListRelationship instances with `datasetId` when the source contains separate datasets, views, or roles. [Example 21](../examples/21-json-dataset-id/example.md) shows dataset-specific Property instances, [example 29](../examples/29-csv-multi-attribute-relationship/example.md) shows a multi-attribute Relationship carrying a flight's departure and arrival airports as two `datasetId`-tagged instances, and [example 12](../examples/12-csv-list-relationship/example.md) shows a ListRelationship.

## Why validation uses simplified

Smart Data Models publishes many JSON Schemas for the key-values representation. Its [payload-validation guideline](https://smartdatamodels.org/index.php/guidelines/#payload-validation) describes schemas that validate values such as a number for `temperature` or an array of URIs for a relationship. Those schemas do not describe the normalized wrappers and metadata.

For a producer, this means that a key-values relationship array cannot be copied directly into normalized output. Cassiopeia maps it to the NGSI-LD construct selected by the mapping, then validates the simplified form against the schema. The [validation guide](../validation.md) explains how Cassiopeia locates and applies those schemas.
