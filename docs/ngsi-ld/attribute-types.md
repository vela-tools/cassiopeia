# NGSI-LD attribute types

An NGSI-LD entity has an `id`, a `type`, and a set of attributes. Each attribute uses one of eight types. The type determines the payload shape, the member that holds its value, the metadata it may carry, and how consumers should read it. This page covers all eight types, their ETSI GS CIM 009 v1.9.1 clauses, their normalized payloads, and the choices that are easy to get wrong. The metadata section covers `observedAt`, `unitCode`, and `datasetId`, along with members that a broker assigns after a producer writes an entity.

The examples below are based on worked examples in this repository. Where a type has a complete example, the page links to it so you can reproduce the mapping.

This page focuses on the types themselves. The [data-representation guide](representation.md) compares normalized, concise, and simplified entities. The [mapping guide](../mapping.md#use-ngsi-ld-attribute-types) shows how to declare the types, and the [data-model guide](../data-models.md) explains how to choose the model that owns an entity's attributes.

The payloads here use the **normalized** form, where each attribute's type token and qualifiers are visible. Cassiopeia uses normalized representation by default. The [data-representation guide](representation.md) shows how the same attributes look in concise and simplified form.

## The eight types at a glance

`type` is optional in a mapping and defaults to `Property`. The ETSI representation rules allow `datasetId` on Property and Relationship attributes, including their specialized types. That covers all eight types listed here. `observedAt` can also accompany each of these attribute types.

| Type | Clause | Value member | Holds |
| --- | --- | --- | --- |
| `Property` | 4.5.2 | `value` | A value of any JSON type |
| `Relationship` | 4.5.3 | `object` | One or more target entity URIs |
| `GeoProperty` | 4.7 | `value` | A GeoJSON geometry |
| `LanguageProperty` | 4.5.18 | `languageMap` | One string per language tag |
| `VocabProperty` | 4.5.20 | `vocab` | A vocabulary IRI |
| `ListProperty` | 4.5.21 | `valueList` | An ordered list of values |
| `ListRelationship` | 4.5.22 | `objectList` | An ordered list of target URIs |
| `JsonProperty` | 4.5.24 | `json` | An arbitrary JSON value, verbatim |

## Property

A `Property` is the base attribute type (ETSI GS CIM 009 v1.9.1 clause 4.5.2). It holds any JSON value, including a string, number, boolean, object, or array. It may also carry `unitCode`, `observedAt`, `datasetId`, and nested sub-attributes. A mapping attribute with no `type` produces a `Property`.

Its value member is `value`. This temperature reading from [example 14](../examples/14-xml-unit-code/example.md) carries a unit and an observation time:

```json
"temperature": {
    "type": "Property",
    "value": 23.0,
    "observedAt": "2026-08-21T10:00:00Z",
    "unitCode": "CEL"
}
```

and a plain one with no metadata:

```json
"stationName": {
    "type": "Property",
    "value": "LJUBLJANA/BEZIGRAD"
}
```

**When to use it.** Use a `Property` for an ordinary measured or descriptive value. Choose `GeoProperty` for geometry, `VocabProperty` for a controlled-vocabulary term, `ListProperty` for an ordered list, and `JsonProperty` for an object whose internal shape the entity does not model. A plain `Property` is also appropriate when the value has meaning only within its own entity.

Produce it with Cassiopeia: the default `type`, or `type: "Property"`; see the [mapping guide](../mapping.md#property).

## Relationship

A `Relationship` links an entity to a target URI (ETSI GS CIM 009 v1.9.1 clause 4.5.3), held in its `object` member. One attribute name may carry several `Relationship` instances that differ by `datasetId`. Together, those instances form a multi-attribute (clause 4.5.5). Each instance has its own `object`, as [example 29](../examples/29-csv-multi-attribute-relationship/example.md) shows for a flight's departure and arrival airports. A Relationship may also include `objectType` and `observedAt`. It is an NGSI-LD attribute that a broker can use to follow the entity graph, not just a foreign-key string.

A Relationship may carry nested sub-attributes. A sub-attribute is the serialization of a Property or one of its subclasses, or of a Relationship (clause 4.5.2.2 with clause 4.5.3). A Relationship can therefore contain another Relationship in its `properties`, alongside nested Properties. [Example 30](../examples/30-csv-nested-relationship/example.md) gives a movie's `hasLeadActor` a nested `playsCharacter` relationship and a `billingOrder` property.

Its value member is `object`. This link from the `ExoPlanet` in [example 20](../examples/20-csv-at-context/example.md) points to the host star:

```json
"hostStar": {
    "type": "Relationship",
    "object": "urn:ngsi-ld:Star:TOI-5789",
    "objectType": "Star"
}
```

**When to use it.** Use a `Relationship` when the value points to another entity with its own `id`. Use a `Property` or `VocabProperty` when the value belongs to the current entity. For several targets on one attribute, use `ListRelationship` for an ordered collection, or use multiple `Relationship` instances with `datasetId` when the targets represent independent roles. [Example 29](../examples/29-csv-multi-attribute-relationship/example.md) gives a flight one `servesAirport` name holding a departure and an arrival instance.

Produce it with Cassiopeia: `type: "Relationship"` with a `target.entity`; see the [mapping guide](../mapping.md#relationship).

## ListRelationship

A `ListRelationship` links one entity to many others through one attribute (ETSI GS CIM 009 v1.9.1 clause 4.5.22). It carries an ordered `objectList` of URIs and may include one shared `objectType`. It represents one ordered collection of links, rather than several separate relationship instances. Like the other Property and Relationship types, it can be used as a dataset-specific instance with `datasetId`. It may also carry nested sub-attributes that qualify the list as a whole (clause 4.5.2.2), as [example 30](../examples/30-csv-nested-relationship/example.md) shows with a `castSize` property on a movie's `hasCast` list relationship.

Its value member is `objectList`. From the `Flight` of [example 12](../examples/12-csv-list-relationship/example.md), a route's link to every aircraft model that flies it:

```json
"hasAircraftModel": {
    "type": "ListRelationship",
    "objectList": [
        "urn:ngsi-ld:AircraftModel:773",
        "urn:ngsi-ld:AircraftModel:320",
        "urn:ngsi-ld:AircraftModel:330"
    ],
    "objectType": "AircraftModel"
}
```

**When to use it.** Use a `ListRelationship` when one attribute represents an ordered collection of related entities, such as the aircraft on a route or stops on a line. Do not confuse it with a multi-attribute Relationship. A `ListRelationship` is one ordered list, while a multi-attribute contains several independent instances of the same relationship name, each identified by `datasetId`. The [data-representation guide](representation.md#relationships-and-arrays) explains the difference. [Example 12](../examples/12-csv-list-relationship/example.md) also shows that some published models do not yet describe `ListRelationship`, so its output may fail their schemas.

Produce it with Cassiopeia: `type: "ListRelationship"` with a `target.entity`; see the [mapping guide](../mapping.md#listrelationship).

## GeoProperty

A `GeoProperty` holds a geospatial value (ETSI GS CIM 009 v1.9.1 clause 4.7). Its value is a GeoJSON geometry of exactly one of six types: `Point`, `MultiPoint`, `LineString`, `MultiLineString`, `Polygon`, `MultiPolygon`. GeoJSON's seventh geometry type, `GeometryCollection` (RFC 7946 clause 3.1.8), is deliberately not admitted, so Cassiopeia never emits one: a source carrying a collection produces no GeoProperty unless the mapping asks for it to be folded into one of the six. It may carry `observedAt` and `datasetId`. NGSI-LD treats geometry separately so a broker can index it for spatial queries. A coordinate pair hidden inside a `Property` is not available to those queries.

Its value member is `value`, holding the geometry. From [example 14](../examples/14-xml-unit-code/example.md), a station's location:

```json
"location": {
    "type": "GeoProperty",
    "value": {
        "type": "Point",
        "coordinates": [
            14.5172,
            46.0658
        ]
    }
}
```

**When to use it.** Use a `GeoProperty` for geometry that consumers may query spatially, which is usually the right choice for a location. Use a `Property` only when the coordinates are data rather than a place. A location stored inside a `Property` cannot be found by a spatial query.

Produce it with Cassiopeia: `type: "GeoProperty"` with `transformation: "geometry"` for an existing geometry, or a specific geometry transformation such as `point` to build one from coordinates. When the source's geometry type is not the one the model wants, a sibling `geometry` block names the conversion; see the [mapping guide](../mapping.md#geoproperty).

## LanguageProperty

A `LanguageProperty` holds text in several languages (ETSI GS CIM 009 v1.9.1 clause 4.5.18). Its `languageMap` uses BCP-47 language tags as keys and the translated text as values. This keeps translations together instead of scattering them across fields such as `name_en` and `name_de`.

Its value member is `languageMap`. From the `Region` of [example 8](../examples/08-json-language-property/example.md), a region named in many languages:

```json
"name": {
    "type": "LanguageProperty",
    "languageMap": {
        "en": "Africa",
        "de": "Afrika",
        "fr": "Afrique",
        "es": "África",
        "it": "Africa",
        "ja": "アフリカ",
        "ko": "아프리카",
        "ru": "Африка",
        "pt-BR": "África",
        "zh-CN": "非洲"
    }
}
```

**When to use it.** Use a `LanguageProperty` when one attribute has translations and the set of languages may vary. Use a plain `Property` for one canonical value or a value that has only one language. `languageMap` keys are checked as language tags, so `pt-BR` and `zh-CN` are valid even though they are not valid attribute names.

Produce it with Cassiopeia: `type: "LanguageProperty"` with a `languageMap` block; see the [mapping guide](../mapping.md#languageproperty).

## VocabProperty

A `VocabProperty` holds a term from a controlled vocabulary, identified by IRI (ETSI GS CIM 009 v1.9.1 clause 4.5.20). The IRI is stored under `vocab`, and Cassiopeia validates it as an IRI. An invalid value produces no attribute. This lets systems identify a category, status, or classification globally instead of relying on a local string.

Its value member is `vocab`. From the `UrbanMobilityPoint` of [example 26](../examples/26-csv-vocab-property/example.md), an OpenStreetMap amenity term mapped to the IRI of its tag page:

```json
"category": {
    "type": "VocabProperty",
    "vocab": "https://wiki.openstreetmap.org/wiki/Tag:amenity=charging_station"
}
```

**When to use it.** Use a `VocabProperty` when a shared vocabulary gives the value an IRI. Use a plain `Property` for an opaque code with no shared vocabulary. A `VocabProperty` identifies a concept, while a `Relationship` points to an entity with its own attributes. A fixed classification term is vocabulary data; a record that belongs in the entity graph is a relationship target.

Produce it with Cassiopeia: `type: "VocabProperty"` with a source that resolves to an IRI; see the [mapping guide](../mapping.md#vocabproperty).

## ListProperty

A `ListProperty` holds an ordered list of values (ETSI GS CIM 009 v1.9.1 clause 4.5.21). The list is stored under `valueList`, and its order is part of the meaning. Consumers should read the values by position.

Its value member is `valueList`. From the `BicycleCounter` of [example 24](../examples/24-json-list-property/example.md), one day's twenty-four hourly counts:

```json
"hourlyCounts": {
    "type": "ListProperty",
    "valueList": [
        35,
        17,
        12,
        9,
        21,
        39,
        142,
        312,
        456,
        343,
        232,
        185,
        166,
        189,
        221,
        347,
        648,
        589,
        301,
        325,
        217,
        134,
        92,
        58
    ]
}
```

**When to use it.** Use a `ListProperty` for an ordered sequence such as hourly readings, ranked results, or ordered steps. Use a plain `Property` with an array when order does not matter. Use `ListRelationship` when the elements are links to other entities. In the example above, `valueList[7]` represents the 07:00 count because the list preserves positions.

Produce it with Cassiopeia: `type: "ListProperty"` with `transformation: "array"`; see the [mapping guide](../mapping.md#listproperty-and-jsonproperty).

## JsonProperty

A `JsonProperty` holds an arbitrary JSON value unchanged (ETSI GS CIM 009 v1.9.1 clause 4.5.24). The source value may be an object, array, or nested combination, and it is stored under `json` without being split into NGSI-LD attributes. This type is useful when the value is a JSON document or its shape varies between records.

Its value member is `json`. In the `WeatherAlert` from [example 25](../examples/25-geojson-json-property/example.md), an open-ended `parameters` block stays intact:

```json
"rawParameters": {
    "type": "JsonProperty",
    "json": {
        "AWIPSidentifier": [
            "SPSAMA"
        ],
        "WMOidentifier": [
            "WWUS84 KAMA 260713"
        ],
        "NWSheadline": [
            "A STRONG THUNDERSTORM WILL IMPACT SOUTHEASTERN BEAVER COUNTY THROUGH 245 AM CDT"
        ],
        "eventMotionDescription": [
            "2026-08-26T07:13:00-00:00...storm...333DEG...16KT...36.82,-100.04"
        ],
        "maxWindGust": [
            "55 MPH"
        ],
        "maxHailSize": [
            "0.25"
        ],
        "BLOCKCHANNEL": [
            "EAS",
            "NWEM",
            "CMAS"
        ],
        "EAS-ORG": [
            "WXR"
        ]
    }
}
```

**When to use it.** Use a `JsonProperty` when a value changes shape between records or is an opaque document that the consumer parses. Prefer real attributes, a nested `Property` built with `mappings`, or top-level attributes when the structure is stable and should be queried. The inner keys of a `JsonProperty` are not NGSI-LD attributes and cannot be targeted by NGSI-LD queries. A custom JSON Schema can still validate the raw JSON when that is useful.

Produce it with Cassiopeia: `type: "JsonProperty"` with `transformation: "object"`; see the [mapping guide](../mapping.md#listproperty-and-jsonproperty).

## Attribute metadata

An attribute may also carry qualifiers that describe its value. NGSI-LD reserves several names for these qualifiers, and Cassiopeia writes them next to the value rather than as nested attributes.

### observedAt

`observedAt` records when a value was observed. The standard defines it as a temporal Property that can describe when a Property or Relationship became valid or was observed. Cassiopeia allows it on all eight attribute types. In [example 14](../examples/14-xml-unit-code/example.md), it appears on the temperature reading:

```json
"temperature": {
    "type": "Property",
    "value": 23.0,
    "observedAt": "2026-08-21T10:00:00Z",
    "unitCode": "CEL"
}
```

An `observedAt` anywhere in an entity marks the entity as temporal. Successive readings with the same identity are then kept as separate observations instead of overwriting one another. A mapping declares it under an attribute's `properties`; because the name is reserved, Cassiopeia emits it as a qualifier rather than a nested Property.

### unitCode

`unitCode` records the unit of a `Property` value, and it is valid only on `Property`. The value is a UN/CEFACT Common Code, not the printed unit symbol: `CEL` means degree Celsius, `MTS` metre per second, and `A97` hectopascal. The temperature in example 14 carries `"unitCode": "CEL"`. When a unit has no Common Code, Cassiopeia leaves `unitCode` out instead of inventing one. [Example 20](../examples/20-csv-at-context/example.md) uses that rule for radius in Earth radii.

### datasetId and multi-attributes

`datasetId` identifies one instance when an attribute name carries several values (ETSI GS CIM 009 v1.9.1 clause 4.5.5). A _multi-attribute_ is serialized as a JSON array of attribute objects, with each instance identified by its own `datasetId`. At most one instance may omit `datasetId`; that is the default instance. The rule applies to Property and Relationship attributes, including specialized types such as `ListProperty` and `ListRelationship`.

From the `WeatherForecast` of [example 21](../examples/21-json-dataset-id/example.md), one forecast variable carrying one instance per numerical model:

```json
"temperatureMax": [
    {
        "type": "Property",
        "value": 22.1,
        "observedAt": "2026-08-25T00:00:00Z",
        "unitCode": "CEL",
        "datasetId": "urn:ngsi-ld:dataset:model:ecmwf-ifs025"
    },
    {
        "type": "Property",
        "value": 23.4,
        "observedAt": "2026-08-25T00:00:00Z",
        "unitCode": "CEL",
        "datasetId": "urn:ngsi-ld:dataset:model:gfs-global"
    },
    {
        "type": "Property",
        "value": 22.8,
        "observedAt": "2026-08-25T00:00:00Z",
        "unitCode": "CEL",
        "datasetId": "urn:ngsi-ld:dataset:model:icon-eu"
    }
]
```

A mapping creates this shape with an `instances` block. Each entry reads its own value and declares its own `datasetId`, while the attribute supplies the shared `type`, `transformation`, and qualifiers. The mechanism applies to Property and Relationship attributes, including specialized types: [example 21](../examples/21-json-dataset-id/example.md) works it on a Property and [example 29](../examples/29-csv-multi-attribute-relationship/example.md) on a Relationship, where each instance's `source` is an object id rather than a value. Use `ListRelationship` when several links belong to one ordered list, and use separate dataset-specific instances when the lists represent different versions or roles. Clause 4.5.5 does not allow `observedAt` or `unitCode` themselves to be multi-attributes.

### instanceId, createdAt, and modifiedAt

Three members may appear when an entity is read from a Context Broker, but a producer never writes them:

- `instanceId` uniquely identifies one instance of a multi-attribute. The broker assigns it; Cassiopeia does not.
- `createdAt` and `modifiedAt` are **system-generated timestamps maintained by the Context Broker**. They record when the broker first stored and last changed the attribute. They differ from `observedAt`, which comes from the data and records a domain event. Cassiopeia writes none of these broker-managed members, so an entity it emits has no `instanceId`, `createdAt`, or `modifiedAt` until a broker stores it.

## Related pages

- [NGSI-LD data representations](representation.md): the same attributes in normalized, concise, and simplified form, and why validation uses the simplified form.
- [Concepts](../concepts.md): where attributes fit in an entity and in the pipeline.
- [Choosing a data model](../data-models.md): which model owns an entity's attributes and when a custom model makes sense.
- [Write a mapping](../mapping.md#use-ngsi-ld-attribute-types): how to declare these types.
