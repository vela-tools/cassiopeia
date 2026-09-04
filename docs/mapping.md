# Write a mapping

A mapping answers one question: what should one input record become? It is a JSON5-compatible document that names the target entity type, derives an entity ID, and turns record values into NGSI-LD attributes.

A mapping does not describe where the input comes from or where the output goes. Run one directly from the command line, or put it in a manifest when you want to repeat or schedule the work. The manifest is optional, but it packages the mapping with its inputs and run settings.

## Start with the result

Suppose a source produces records with this shape:

~~~json
{
    "id": 7,
    "name": "Main station",
    "temperature": "21.5"
}
~~~

The smallest useful mapping for that record is:

~~~json5
{
    version: "v4",
    dataModel: "Sensor",
    identity: {
        entityName: "Station-{{ id }}",
    },
    attributes: {
        name: {
            source: "{{ name }}",
        },
        temperature: {
            source: "{{ temperature }}",
            transformation: "float",
        },
    },
}
~~~

The mapping creates a `Sensor` entity with an ID based on the record's `id` field. It copies `name` as an attribute and converts `temperature` from text to a number.

Cassiopeia applies the mapping once to each record. If the source contains several records, it resolves the resulting fragments into entities.

## The document shape

JSON5 is the recommended format for mapping files. It keeps configuration documents readable. Comments can explain an unusual source field or modeling decision, trailing commas make edits and diffs less error-prone, and unquoted keys reduce visual noise. Cassiopeia also accepts strict JSON because JSON is a valid subset of JSON5. Use the `.json5` extension for mappings that use JSON5 features. A strict JSON file can use `.json` when another tool requires it.

Every mapping has four top-level parts:

~~~json5
{
    version: "v4",
    dataModel: "Sensor",
    identity: {
        entityName: "Station-{{ id }}",
    },
    attributes: {
        // Attribute declarations go here.
    },
}
~~~

`version` identifies the mapping syntax. Cassiopeia currently accepts `v4`.

`dataModel` names the entity type the mapping produces. It can be an unqualified model name such as `Sensor` or a repository-qualified name such as `dataModel.OCF/Sensor`. A qualified name lets Cassiopeia find a published Smart Data Model's schema. An unqualified name declares a custom model without such a lookup. The [data-model guide](data-models.md) explains how to choose between a published model and one of your own.

`identity` tells Cassiopeia how to build the entity ID and, optionally, its scope.

`attributes` is an object whose keys are the names of the attributes in the output entity. Each value is an attribute declaration.

Input paths, output destinations, validation policy, and schedules do not belong in this document. Set them on the command line or in a manifest.

## Choose the entity identity

The `entityName` field is required. Its value can be a literal, a source field, or a template built from several parts:

~~~json5
identity: {
    entityName: "Station-{{ station_id }}",
}
~~~

For a record whose `station_id` is `A-17`, Cassiopeia builds an ID from `Station-A-17` and the mapping's entity type.

The identity should describe the thing represented by the entity. Use a stable source identifier when the source has one. Avoid using a changing measurement as the identity unless each measurement represents a different entity.

An identity can also include one scope or several scopes:

~~~json5
identity: {
    entityName: "Station-{{ station_id }}",
    scope: "/{{ city }}",
}
~~~

For multiple scopes, use an array of templates:

~~~json5
identity: {
    entityName: "Station-{{ station_id }}",
    scope: [
        "/{{ country }}",
        "/{{ city }}",
    ],
}
~~~

Cassiopeia resolves scopes per record. An empty or missing scope value does not add a scope to the entity.

## Map an attribute

An attribute declaration normally starts with a `source` field:

~~~json5
attributes: {
    name: {
        source: "{{ name }}",
    },
    capacity: {
        source: "{{ capacity }}",
        transformation: "integer",
    },
}
~~~

The key on the left, such as `name` or `capacity`, becomes the NGSI-LD attribute name. The `source` expression tells Cassiopeia where the value comes from. The optional `transformation` converts it before Cassiopeia builds the entity.

In mapping syntax, an attribute without `type` is a `Property`. Set `type` explicitly for a relationship, geometry, language map, vocabulary, list, or JSON property.

### Source values

A source can be a literal:

~~~json5
source: "Ljubljana"
~~~

It can refer to a field in the current record:

~~~json5
source: "{{ name }}"
~~~

It can follow a path through nested objects:

~~~json5
source: "{{ properties.name }}"
~~~

For field names that are not valid template identifiers, use bracket notation through `this`:

~~~json5
source: "{{ this['CO(GT)'] }}"
~~~

The special `{{ context }}` expression resolves to the complete current record. Use it when a transformation needs the whole source object, such as when passing an already structured geometry or JSON value through a mapping.

You can provide several source parts as an array:

~~~json5
source: [
    "{{ street }}",
    " ",
    "{{ house_number }}",
],
transformation: "string",
~~~

For a text transformation, Cassiopeia concatenates the resolved parts in order. Include separators such as a space in the source array when needed.

A simple reference to a missing field resolves to `null`. Output settings, not the mapping, determine whether Cassiopeia omits or writes the resulting null attribute.

### Templates

Templates can combine literal text and fields:

~~~json5
source: "{{ street }} {{ house_number }}, {{ city }}"
~~~

They can also contain conditional expressions:

~~~json5
source: "{% if status == 'active' %}available{% else %}offline{% endif %}"
~~~

Filters transform a value inside a template:

~~~json5
source: "{{ name | clean }}"
~~~

Use templates for values that depend on the record. A literal is easier to read and less likely to hide a mapping mistake when the value never changes.

### More filters and functions

Templates support more than plain text. Cassiopeia's `get` reads a map entry, and `date_subtract_seconds` shifts a timestamp. Mathematical functions such as `sqrt`, `hypot`, `bearing`, and `wind_speed` derive quantities from source numbers. `dms_point` and `geohash` build geospatial values. A computation with no finite answer, such as the square root of a negative or an inverse sine out of range, resolves to nothing and drops the attribute instead of failing the record. The [template guide](templates.md) is the full reference for these functions, the surrounding Tera syntax, and the contributed filters and tests Cassiopeia enables.

## Convert values

The transformation determines the value Cassiopeia builds. If you omit it, Cassiopeia treats the value as a string. Use an explicit transformation when the target model expects another type.

| Transformation | Result |
| --- | --- |
| `boolean` | A boolean value. |
| `integer` | A signed integer. |
| `float` | A floating-point number. Comma decimal separators are accepted. |
| `string` | Text. Several source parts are concatenated. |
| `array` | An array containing the source parts. Nested source arrays are flattened. |
| `object` | A JSON object parsed from the source value. |
| `datetime` | A date and time value. |
| `date` | A date value. |
| `time` | A time value. |
| `geometry` | An existing GeoJSON geometry, whichever of the six admissible types it is. A `GeometryCollection` yields no attribute. |
| `point` | A GeoJSON `Point`. |
| `multipoint` | A GeoJSON `MultiPoint`. |
| `linestring` | A GeoJSON `LineString`. |
| `multilinestring` | A GeoJSON `MultiLineString`. |
| `polygon` | A GeoJSON `Polygon`. |
| `multipolygon` | A GeoJSON `MultiPolygon`. |

`datetime`, `date`, and `time` read the spellings sources actually write. Supported values include RFC 3339 (`2026-03-01T11:04:35Z`, `2026-03-01T11:04:35+02:00`), the same instant with a space instead of the `T` and with or without a UTC offset (`2026-03-01 11:04:35`, `2026-03-01 11:04:35+00:00`, `2026-03-01 11:04:35+0000`), `Y-m-d` and `d/m/Y` dates with `:` or `.` between the time parts, a bare date, and a Unix epoch in seconds, milliseconds, or nanoseconds. Fractional seconds are preserved, offsets are applied, and a value without a zone is read as UTC. Unreadable text is not dropped quietly: Cassiopeia omits the attribute and warns with the attribute name and the text it could not read. An `observedAt` sub-property follows the same rules. If it cannot be read, the attribute is still published without that qualifier.

The transformation and the NGSI-LD attribute type answer different questions. `transformation: "float"` says how to parse the value. `type: "Property"` says how to represent it in the entity. A geometry attribute therefore commonly uses both:

~~~json5
location: {
    type: "GeoProperty",
    source: "{{ geometry }}",
    transformation: "geometry",
}
~~~

## Use NGSI-LD attribute types

Cassiopeia supports the NGSI-LD attribute types defined by version 1.9.1 of the specification. The `type` field selects the type produced by a mapping. This section shows how to declare each one. The [attribute-type reference](ngsi-ld/attribute-types.md) provides more detail about each type's canonical payload, value member, and differences from similar types.

### Property

Properties hold values. This is the default mapping type, so ordinary value attributes do not need to declare it:

~~~json5
temperature: {
    source: "{{ temperature }}",
    transformation: "float",
}
~~~

### Relationship

A relationship points to another entity. Its `source` supplies the target entity name, and `target.entity` supplies the target model:

~~~json5
controlledAsset: {
    type: "Relationship",
    source: "{{ building_id }}",
    target: {
        entity: "Building",
    },
}
~~~

For a record whose `building_id` is `B-12`, this relationship targets the corresponding `Building` entity. To give one relationship name several targets that differ by role, declare `instances` instead of a single `source`. See [multiple instances](#carry-several-instances-under-one-name) and [example 29](examples/29-csv-multi-attribute-relationship/example.md). A relationship can also carry its own `properties`, including a nested relationship. See [attribute metadata](#add-attribute-metadata) and [example 30](examples/30-csv-nested-relationship/example.md).

### GeoProperty

GeoProperties hold GeoJSON geometry. Use `geometry` when the source already contains a complete geometry object. Use one of the specific geometry transformations when the source contains coordinates that need to become a particular geometry type.

~~~json5
location: {
    type: "GeoProperty",
    source: "{{ geometry }}",
    transformation: "geometry",
}
~~~

A GeoProperty holds exactly six geometry types, the ones NGSI-LD admits (ETSI GS CIM 009 v1.9.1 clause 4.7): `Point`, `MultiPoint`, `LineString`, `MultiLineString`, `Polygon`, `MultiPolygon`. GeoJSON's seventh type, `GeometryCollection` (RFC 7946 clause 3.1.8), is not one of them. A source that carries one produces no GeoProperty unless the mapping asks for it to be folded, described below.

Cassiopeia normalises every geometry it emits to RFC 7946's producer rules. It closes polygon rings that are not closed and rewinds every ring to the right-hand rule in clause 3.1.6: exterior rings counterclockwise and holes clockwise. A position's optional third element, its altitude (clause 3.1.1), is carried through.

#### Convert between geometry types

The `transformation` names the geometry type the attribute should hold. When the source already carries that type, or a type that reaches it without discarding anything, the attribute is built and nothing needs to be declared. Three conversions are lossless and therefore automatic:

- **identity**: the source is already the declared type;
- **promotion**: a `Point` becomes a `MultiPoint`, a `LineString` a `MultiLineString`, and a `Polygon` a `MultiPolygon`;
- **unwrapping**: a multi-geometry carrying exactly one member becomes that member.

Everything else discards coordinates, and Cassiopeia never discards geometry without the mapping saying so. A `MultiPolygon` of two surfaces asked for a `Polygon` throws one surface away; a `Polygon` asked for a `Point` throws away its whole extent. Those conversions are refused unless a sibling `geometry` block names the loss:

~~~json5
location: {
    type: "GeoProperty",
    transformation: "polygon",
    geometry: {
        convert: "largest",
    },
    source: "{{ geometry }}",
}
~~~

When a conversion is refused, the attribute is dropped, the entity is still written, and the run counts one warning per record and prints one message naming the attribute and the reason.

The `geometry` block takes three keys:

| Key | Values | Default |
| --- | --- | --- |
| `convert` | one of the conversions below | absent, meaning lossless conversions only |
| `winding` | `rfc7946`, `keep` | `rfc7946` |
| `altitude` | `keep`, `drop` | `keep` |

`winding: "keep"` emits rings exactly as the source wound them. `altitude: "drop"` truncates every position to a longitude and a latitude.

#### The conversions

**Pick one member of a multi-geometry**, coordinates untouched:

| `convert` | Effect |
| --- | --- |
| `first` | The first member, in coordinate-array order. |
| `largest` | The member of greatest geodesic area (surfaces) or geodesic length (curves). Not available toward `point` or `multipoint`: a position has no extent to rank by, so a mapping declaring it there is rejected when it loads. |

**Derive a position:**

| `convert` | Effect |
| --- | --- |
| `centroid` | The geometry's centroid. May fall outside a concave surface. |
| `point-on-surface` | A position guaranteed to lie on or inside the geometry. |
| `first-vertex` | The first position the geometry lists, copied verbatim. |
| `bbox-center` | The centre of the geometry's bounding box. |

**Derive a curve:**

| `convert` | Effect |
| --- | --- |
| `exterior-ring` | A surface's exterior ring as a closed curve, discarding its holes. |
| `boundary` | A surface's whole boundary, one curve per ring, exterior first. Loses no coordinate. |
| `connect` | A `MultiPoint`'s positions joined into one curve, in coordinate-array order. |

**Derive a surface:**

| `convert` | Effect |
| --- | --- |
| `ring` | A closed curve read as a surface's exterior ring. The curve must already close and hold at least four positions; an open curve is refused, never closed for you. |
| `convex-hull` | The convex hull of the geometry's positions. |
| `envelope` | The geometry's bounding box, as a rectangle. |

**Other:**

| `convert` | Effect |
| --- | --- |
| `vertices` | Every distinct position the geometry lists, as a `MultiPoint`. A surface contributes all its rings, not only the exterior one. |
| `flatten` | Folds a `GeometryCollection` into one geometry: a lone member becomes that geometry, members of one dimension become the matching multi-geometry, and a collection mixing dimensions is refused. |

`centroid` and `point-on-surface` are computed in the plane, using longitude and latitude as plane coordinates, so they drift at high latitude and are meaningless across the antimeridian. `largest` and the winding decision are geodesic and have no such caveat. A conversion guarantees the result is a structurally valid RFC 7946 geometry, but not a topologically sensible one. For example, `ring` on a self-intersecting curve yields a valid polygon bounding a nonsensical surface.

Conversions preserve a position's altitude wherever they can: the ones that select or regroup existing positions (`first`, `largest`, `first-vertex`, `exterior-ring`, `boundary`, `connect`, `ring`, `vertices`, `flatten`) carry it through, and the ones that compute new coordinates (`centroid`, `point-on-surface`, `bbox-center`, `convex-hull`, `envelope`) do not.

A conversion that could never produce the declared type is rejected when the mapping loads, naming the attribute, rather than failing on every record.

When a geometry has to be converted inside a template, for a value that a `transformation` cannot reach, use the [`geo_convert` function](templates.md#functions).

### LanguageProperty

A LanguageProperty contains one value per language. Declare those values under `languageMap`:

~~~json5
description: {
    type: "LanguageProperty",
    languageMap: {
        en: {
            source: "{{ description_en }}",
        },
        sl: {
            source: "{{ description_sl }}",
        },
    },
}
~~~

The keys under `languageMap` are language tags. Each language entry is an attribute declaration, so it can use its own source and transformation.

### VocabProperty

A VocabProperty stores a vocabulary IRI. Map the IRI with a source expression and set the type explicitly:

~~~json5
vehicleType: {
    type: "VocabProperty",
    source: "{{ vehicle_type }}",
}
~~~

The resolved source must be a well-formed IRI. A value that is not an IRI produces no attribute. [Example 26](examples/26-csv-vocab-property/example.md) builds the IRI by interpolating an OpenStreetMap amenity term into its canonical tag-page URL.

### ListProperty and JsonProperty

Use `ListProperty` for an ordered list of values. Use `JsonProperty` for a JSON value that should remain JSON in the NGSI-LD entity:

~~~json5
supportedModes: {
    type: "ListProperty",
    source: "{{ modes }}",
    transformation: "array",
},
metadata: {
    type: "JsonProperty",
    source: "{{ metadata }}",
    transformation: "object",
}
~~~

The source shape and transformation must match the data the source provides. A string containing JSON is not automatically an object. The mapping must ask Cassiopeia to parse it as one. A bare field reference such as `{{ modes }}` resolves to the record's actual value, so an array field stays an array and an object field stays an object. [Example 24](examples/24-json-list-property/example.md) carries an ordered array as a `ListProperty`, while [example 25](examples/25-geojson-json-property/example.md) keeps a variable object whole as a `JsonProperty`.

### ListRelationship

ListRelationship represents links to multiple entities of the target model. Use it when the source describes a collection of related entity identifiers. Its target declaration identifies the model of those related entities:

~~~json5
hasAircraftModel: {
    type: "ListRelationship",
    source: "{{ equipment_codes }}",
    target: {
        entity: "AircraftModel",
    },
}
~~~

The source is read as a collection of identifiers with no `split`. An array contributes one target per element, while a string contributes one target per whitespace- or comma-separated token. For example, `"744 777"` links to both `AircraftModel:744` and `AircraftModel:777`. Like a Relationship, a ListRelationship may appear as several `datasetId`-tagged instances under one attribute name. See [multiple instances](#carry-several-instances-under-one-name). It may also carry its own `properties` as sub-attributes that qualify the whole list, as [example 30](examples/30-csv-nested-relationship/example.md) does with a `castSize` property. [Example 12](examples/12-csv-list-relationship/example.md) shows a complete case, including the tradeoff against a published model that predates the type.

## Build nested values

Use `mappings` when an attribute contains a structured object whose fields need their own source expressions:

~~~json5
address: {
    type: "Property",
    transformation: "object",
    mappings: {
        streetAddress: {
            source: "{{ street }}",
        },
        addressLocality: {
            source: "{{ city }}",
        },
    },
}
~~~

Nested declarations follow the same rules as top-level attributes. They can have their own transformations, nested mappings, and metadata.

Use nested mappings for a known output structure. Use `{{ context }}` with an object transformation when the source already contains the complete object and you do not need to rename or transform its fields individually.

## Add attribute metadata

The `properties` member declares attributes of an attribute. It is different from the top-level `attributes` object:

~~~json5
temperature: {
    source: "{{ temperature }}",
    transformation: "float",
    properties: {
        observedAt: {
            source: "{{ timestamp }}",
            transformation: "datetime",
        },
        unitCode: {
            source: "CEL",
        },
    },
}
~~~

`observedAt`, `unitCode`, and `datasetId` are handled as NGSI-LD attribute qualifiers. Any other entry under `properties` becomes a nested sub-attribute with the declared `type`, or a `Property` when no type is specified. It can also be a `VocabProperty`, a `LanguageProperty`, or a `Relationship`. A sub-attribute is the serialization of a Property or any of its subclasses, and of a Relationship (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with clause 4.5.3). As a result, `properties` can declare nested relationships as well as nested properties, recursively to any depth. [Example 30](examples/30-csv-nested-relationship/example.md) hangs a `playsCharacter` relationship off a `hasLeadActor` relationship. A mapping can therefore represent both standard qualifiers and model-specific attribute metadata.

Cassiopeia resolves metadata from the same source record as its parent attribute. For a temporal mapping, put the timestamp on the attributes that represent observations. Cassiopeia can then keep successive observations associated with the same entity identity.

## Carry several instances under one name

One attribute name can hold several instances that differ by `datasetId`, a _multi-attribute_ (ETSI GS CIM 009 v1.9.1 clause 4.5.5). Declare them with an `instances` list instead of a single `source`. The attribute supplies the shared `type`, `transformation`, `target`, and attribute-level `properties`. Each entry supplies only what makes it distinct: its own `source` and `datasetId`:

~~~json5
temperatureForecast: {
    type: "Property",
    transformation: "float",
    properties: {
        unitCode: {
            source: "CEL",
        },
    },
    instances: [
        {
            source: "{{ ecmwf_temp }}",
            properties: {
                datasetId: {
                    source: "urn:ngsi-ld:dataset:model:ecmwf",
                },
            },
        },
        {
            source: "{{ gfs_temp }}",
            properties: {
                datasetId: {
                    source: "urn:ngsi-ld:dataset:model:gfs",
                },
            },
        },
    ],
}
~~~

The result is serialized as a JSON array of attribute objects, one per instance. At most one instance may omit its `datasetId`; that instance is the default.

`instances` works on every reified attribute kind. Each instance's `source` provides the value that kind reads from a single source:

- On a **Property** or any of its subtypes (`GeoProperty`, `LanguageProperty`, `VocabProperty`, `ListProperty`, `JsonProperty`), the `source` is a value. An instance with no value contributes nothing, so a model that reports no value for a record produces no instance. [Example 21](examples/21-json-dataset-id/example.md) carries several numerical-model forecasts under one Property name.
- On a **Relationship**, the `source` is the target object's ID, and every instance shares the attribute-level `target`. [Example 29](examples/29-csv-multi-attribute-relationship/example.md) gives a flight one `servesAirport` name with departure and arrival instances.
- On a **ListRelationship**, the `source` is an object-ID list, tokenized as it is for a plain list relationship. Each instance becomes its own `objectList` under its own `datasetId`.

`observedAt` and `unitCode` are qualifiers themselves, so clause 4.5.5 does not allow them to be multi-attributes. They may appear only inside an instance's shared or per-instance `properties`.

## Emit a related entity

Sometimes a record contains enough information to describe a related entity as well as the main one. Add `syntheticEntity` to the attribute that links the entities:

~~~json5
owner: {
    type: "Relationship",
    syntheticEntity: {
        dataModel: "Organization",
        identity: {
            entityName: "{{ owner_id }}",
        },
        attributes: {
            name: {
                source: "{{ owner_name }}",
            },
        },
    },
}
~~~

The synthetic entity uses the same source record but has its own data model, identity, and attributes. Cassiopeia emits it alongside the main entity and links it through the attribute that declares it. The nested mapping inherits the document version, so it does not need its own `version` field.

Use a synthetic entity when the source record is the only place where the related entity can be described. A relationship to an entity defined by another source does not need a synthetic entity. It only needs a target declaration.

## Keep mapping and execution separate

A mapping should stay focused on the record-to-entity transformation. Put these concerns outside it:

- Put the input path or URL in a CLI invocation or manifest.
- Put the output directory or context broker in output settings.
- Put validation strictness in output or command settings.
- Put schedules, retries, and failure policy in a manifest.
- Put general defaults in the configuration file and its overrides.

This separation lets the same mapping run against different inputs, destinations, and schedules without changing the transformation.

## Check a mapping

Start with one record and one or two attributes. Run the mapping before adding nested structures or conditional templates. If the result is not what you expect, check the resolved identity first, then the source path, transformation, and selected NGSI-LD type.

Cassiopeia reads mappings as JSON5, so comments and trailing commas are allowed. The `format` command can reformat a mapping without changing its meaning. Use `.json5` for the recommended format or `.json` when another tool requires strict JSON.

If the mapping loads but an entity fails validation, inspect the output value instead of only the mapping syntax. A schema may require a number where the source provides text, a GeoProperty where the mapping creates a Property, or a particular attribute name. The fix is usually an explicit transformation, type, or source path.

## Next steps

- [Templates](templates.md): see the full filter and function vocabulary for `source` expressions.
- [Source formats](source-formats.md): the records a particular input produces.
- [Choosing a data model](data-models.md): whether to target a published model or one of your own.
- [Output](output.md) and [representations](representation.md): where entities go and the shape they take.
- [Validation](validation.md): how entities are checked against a schema before writing.
- [Manifests](manifest.md): save a complete run for reuse or scheduling.
