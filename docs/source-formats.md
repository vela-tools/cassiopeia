# Source formats

Cassiopeia converts each supported source format into a sequence of records before applying a mapping. Each format produces a different record shape, so start by checking what Cassiopeia puts in a record before writing the mapping.

Most inputs can use automatic format detection. Cassiopeia inspects the source and selects the most specific supported format it recognizes. Declare the format when the source has no useful extension, its content is ambiguous, or you want the run configuration to make the choice explicit.

The format can be set on a manifest input with `format`, or through the equivalent command-line option. The accepted values are `csv`, `json`, `geojson`, `kml`, `kmz`, `xml`, `shapefile`, and `grib`.

## At a glance

| Format | Records | Important source shape | Collections |
| --- | --- | --- | --- |
| CSV | One record per data row | Column names become fields | No |
| JSON | One record per object in a top-level array | The object is used as-is | No |
| GeoJSON | One record per feature | `id`, `properties`, `geometry`, and `bbox` | No |
| KML or KMZ | One record per placemark | `properties` and `geometry` | Folders become collections |
| XML | Determined by the repeated-child convention | Folded element tree with `@` and `#` keys | No |
| ESRI Shapefile | One record per feature | `properties` and WGS84 `geometry` | Multiple layers become collections |
| GRIB | One record per unmasked grid cell | Coordinates, times, level, and parameter columns | No |

The collection column matters when a source contains several logical groups that need different mappings. KML folders and multi-layer Shapefile archives are currently the only formats that provide collections.

## Automatic detection and explicit format

Automatic detection uses the source content. A JSON document that is also valid GeoJSON is classified as GeoJSON rather than generic JSON. Cassiopeia checks for KML before generic XML and distinguishes a Shapefile archive from other ZIP files by its contents. It identifies GRIB from its binary marker and reads the edition from the header.

Use an explicit format when detection does not have enough context. For example, a small or unusual delimited file may need to be declared as CSV. A source downloaded without a meaningful filename may also need an explicit format in the run configuration.

The format declaration selects the ingestor but does not change the source data. The mapping must still use the record shape that the ingestor produces.

## Inspecting a source

The `profile` command uses the same detection as a mapping run and prints the result without reading the whole source or writing anything. Use it to confirm which ingestor a file selects. For formats that expose parsing details, it also shows those details before you write a mapping or decide to declare the format explicitly.

~~~bash
cassiopeia profile energy-1m.csv
~~~

~~~text
energy-1m.csv · 127.5 MiB
CSV · text/csv · 90% confidence
delimiter , · quote " · header row · CRLF · UTF-8
~~~

The report has at most three lines. The first names the file and gives its size. The second shows the detection result: the format, its media type, and a confidence score. The third appears only when the format has detectable parsing details. For CSV, it shows the dialect above, including the delimiter, quoting, header presence, line terminator, and character encoding. For GRIB, it shows the edition. Formats that need no such parameters, including JSON, GeoJSON, KML, XML, and Shapefile, print only the first two lines.

~~~text
stations.geojson · 4.2 KiB
GeoJSON · application/geo+json · 100% confidence
~~~

The report goes to standard output, so redirection captures it intact. Cassiopeia adds colour only when the output is an interactive terminal. The report describes the format and its parsing, not the record shape. To see the fields a mapping will address, map a small sample and inspect one record.

## CSV

Cassiopeia treats a CSV source as a table. The header row supplies the field names, and each following row becomes one record. A CSV source is always a single collection, so its records do not carry a collection label.

### Dialect detection

Cassiopeia profiles the CSV before reading it. It detects the delimiter, whether a header is present, the quote character, and whether lines end with LF or CRLF. The profiler considers comma, tab, semicolon, pipe, and space delimiters, along with double quotes, single quotes, or no quoting.

If a header contains accented characters, Cassiopeia transliterates them when building the field name. It replaces newlines in a header with spaces. Because this cleanup changes the original header, do not assume it is a reliable template path. Inspect the profiled record shape when the source uses unusual headers.

### Value inference

Cassiopeia converts CSV fields to JSON values as it reads them:

- Empty fields become `null`.
- Integers and decimal numbers become JSON numbers.
- `true`, `yes`, and `on` become `true`; `false`, `no`, and `off` become `false`, ignoring case.
- Values with leading zeroes remain strings, which preserves identifiers such as `00123`.
- Everything else remains a string.

This inference provides a useful starting point, but it does not replace an intentional mapping. Use an explicit transformation when the target model requires a particular type or when the value's meaning is not clear from its spelling.

### Mapping a CSV record

For a row such as:

~~~text
station_id,name,temperature
A-17,Main station,21.5
~~~

The mapping can address the fields directly:

~~~json5
identity: {
    entityName: "Station-{{ station_id }}",
},
attributes: {
    name: {
        source: "{{ name }}",
    },
    temperature: {
        source: "{{ temperature }}",
        transformation: "float",
    },
}
~~~

Mappings address named fields, so a headerless file may need preprocessing before it can be mapped cleanly. If Cassiopeia cannot infer the dialect reliably, declare the format explicitly and verify the resulting record shape before writing the mapping.

## JSON

The generic JSON ingestor expects a top-level array of objects. Each object becomes one record, and its nested structure is preserved:

~~~json
[
    {
        "id": "A-17",
        "reading": {
            "value": 21.5
        }
    },
    {
        "id": "A-18",
        "reading": {
            "value": 20.9
        }
    }
]
~~~

A mapping can address the nested value with `{{ reading.value }}`. Cassiopeia does not treat a top-level object as one generic JSON record, and it does not accept an array of scalar items. Wrap the source in an array of objects or use a format whose record convention matches the source.

Generic JSON and GeoJSON use the same syntax, but they have different record conventions. Cassiopeia routes a valid GeoJSON document to the GeoJSON ingestor.

## GeoJSON

Cassiopeia supports GeoJSON `FeatureCollection` documents and single GeoJSON `Feature` documents. Each feature becomes one record. When the corresponding source members are present, the record has this shape:

~~~json
{
    "id": "A-17",
    "properties": {
        "name": "Main station"
    },
    "geometry": {
        "type": "Point",
        "coordinates": [
            14.51,
            46.05
        ]
    },
    "bbox": [
        14.50,
        46.04,
        14.52,
        46.06
    ]
}
~~~

Cassiopeia copies the feature ID to `id`, copies `properties` as an object, and copies `geometry` as a GeoJSON geometry. It also copies `bbox` when present. A geometry-only GeoJSON document is not accepted as a source collection.

RFC 7946 lets a feature's geometry be a `GeometryCollection`, which NGSI-LD does not admit as a GeoProperty value. Such a feature produces no GeoProperty: the attribute is dropped and the run counts a warning, unless the mapping sets `geometry.convert` to `flatten` to fold the collection into one geometry.

The source geometry is ready to use as an NGSI-LD `GeoProperty`:

~~~json5
location: {
    type: "GeoProperty",
    source: "{{ geometry }}",
    transformation: "geometry",
}
~~~

Properties remain under `properties`, so a source field called `name` is addressed as `{{ properties.name }}`. Cassiopeia does not automatically use the feature ID as the NGSI-LD entity ID. Use it explicitly in `identity.entityName` when you want that identity.

## KML and KMZ

KML and KMZ are geospatial document formats, with KMZ being the zipped form of KML. Cassiopeia walks nested `Document` and `Folder` structures and emits one record for each `Placemark`.

A placemark record has the same general shape as a GeoJSON feature:

~~~json
{
    "id": "station-17",
    "properties": {
        "name": "Main station",
        "description": "Central platform",
        "folder": "Stations",
        "temperature": "21.5"
    },
    "geometry": {
        "type": "Point",
        "coordinates": [
            14.51,
            46.05
        ]
    }
}
~~~

Cassiopeia places the placemark's `name`, `description`, and extended data in `properties`. Its `id` attribute becomes `id`, and supported KML geometry becomes GeoJSON under `geometry`. A KML `LinearRing` becomes a GeoJSON `LineString`, matching the source semantics of a standalone ring.

A KML `MultiGeometry` may mix geometry kinds, which GeoJSON's multi-geometries cannot. Cassiopeia folds one into the multi-geometry of its members' family: a single member becomes that geometry, points become a `MultiPoint`, line strings and linear rings a `MultiLineString`, polygons a `MultiPolygon`, and a nested `MultiGeometry` contributes its own members to the same fold. A `MultiGeometry` mixing families has no GeoJSON equivalent and produces no `geometry` key, exactly like a KML geometry kind GeoJSON does not describe.

### Folders and collections

A placemark inside a folder carries that folder's name as its collection label. Cassiopeia preserves the original folder name, including spaces and capitalization, so a manifest can route different folders to different mappings. It also nests folder data under a snake-case namespace in the record. For example, a folder named `Camera Area` is available through `camera_area`, as in `{{ camera_area.properties.name }}`.

A placemark outside a folder has no collection label. A mapping binding that expects a collection cannot match it.

## XML

Generic XML is deliberately schema-less. Cassiopeia folds the XML tree into JSON-like values and uses one deterministic rule to split records. It does not guess which element represents a record.

### XML value shape

An element with only text becomes a string. An empty element becomes `null`. An element with attributes, children, or processing instructions becomes an object:

~~~xml
<station code="A-17">Main station</station>
~~~

~~~json
{
    "station": {
        "@code": "A-17",
        "#text": "Main station"
    }
}
~~~

XML attributes use an `@` prefix. Text in a mixed-content object uses `#text`, and processing instructions use `#pi`. Cassiopeia keeps namespace prefixes in element and attribute names.

### Record splitting

If the root has exactly one repeated child element directly beneath it, each occurrence becomes a record. For example:

~~~xml
<data>
    <station><id>A-17</id><name>Main station</name></station>
    <station><id>A-18</id><name>North station</name></station>
</data>
~~~

This produces records with `id` and `name` fields, so a mapping can use `{{ id }}` and `{{ name }}` directly.

If the root has no repeated child, the whole root becomes one record. If it has two or more repeated child names, Cassiopeia also keeps the whole root as one record instead of choosing one repeated element arbitrarily. A repeated element under an intermediate wrapper is not used as the split point. When the source structure does not identify a record boundary unambiguously, Cassiopeia preserves the structure for the mapping to handle.

When a repeated child is a scalar rather than an object, its record contains the value under `#text`:

~~~xml
<data><value>21.5</value><value>20.9</value></data>
~~~

Each record therefore has a `#text` member containing `21.5` or `20.9`.

## ESRI Shapefile

A Shapefile source can be a local `.shp` main file with its companion files beside it, or a ZIP bundle containing one or more layers and their companions. Cassiopeia reads the feature geometry and dBase attributes, then emits one record per feature.

Each record has this general shape:

~~~json
{
    "properties": {
        "NAME": "Main station",
        "ACTIVE": true,
        "VALUE": 21.5
    },
    "geometry": {
        "type": "Point",
        "coordinates": [
            14.51,
            46.05
        ]
    }
}
~~~

dBase fields are under `properties`. Cassiopeia converts their values to JSON where possible: character and memo fields become strings, numeric fields become numbers, logical fields become booleans, and date fields become ISO-style date strings. It omits null fields. dBase tooling may uppercase field names, so inspect the actual property names when writing a mapping.

### Geometry and coordinate reference systems

Shapefile geometries are emitted as GeoJSON for use as NGSI-LD `GeoProperty` values. When a `.prj` companion is present, Cassiopeia parses its coordinate reference system and reprojects the geometry to EPSG:4326, using WGS84 longitude and latitude. Without a `.prj` file, it assumes the coordinates are already EPSG:4326 and emits them unchanged.

The `.dbf` companion is required because it carries the feature attributes. A bare `.shp` without its required companion files is not a complete input.

### Multiple layers

A ZIP containing one Shapefile layer produces ordinary records without a collection label. A ZIP containing several `.shp` layers gives each layer a collection label based on its filename without the extension. Use collection-specific manifest bindings when the layers need different mappings.

## GRIB

GRIB is a binary format for gridded meteorological and climate data. Cassiopeia supports GRIB1 and GRIB2 through the single `grib` source format and reads the edition from the file header.

GRIB stores one field per parameter, while an NGSI-LD observation often needs several parameters at one location. Cassiopeia groups fields that share a grid, vertical level, reference time, and forecast time, then pivots them into one record for each unmasked grid cell. The resulting record contains:

- `latitude` and `longitude` as separate numeric fields.
- `level_type` and `level` when the field declares a vertical level (see [Accessing the level](#accessing-the-level)).
- `referenceTime` when present.
- `forecastTime` when present.
- One field for each parameter, keyed by its Cassiopeia-canonical name.

For example, a record may look like:

~~~json
{
    "latitude": 46.0,
    "longitude": 14.0,
    "level_type": "height_above_ground",
    "level": 2,
    "referenceTime": "2026-08-23T00:00:00+00:00",
    "forecastTime": "2026-08-23T00:00:00+00:00",
    "temperature": 285.69,
    "dewpoint": 284.99,
    "humidity": 95.3
}
~~~

Masked parameter values are omitted from a record. A grid cell for which every parameter is masked produces no record.

### Canonical parameter names

Both editions use the same name for a physical quantity, regardless of backend: a short, space-free, level-independent `snake_case` key. The key is meant to be read directly in a template, as in `{{ temperature }}`. A spaced WMO name would require `this['...']` bracket access, while a kebab-case name such as `wind-u` would be parsed as subtraction. The same mapping therefore works across editions and backends. The canonical set is a curated vocabulary. Here are some of the most common keys:

| Canonical key | Meaning | GRIB2 (discipline, category, number) | GRIB1 indicator |
| --- | --- | --- | --- |
| `temperature` | Air temperature | 0, 0, 0 | 11 |
| `dewpoint` | Dew point temperature | 0, 0, 6 | 17 |
| `humidity` | Relative humidity | 0, 1, 1 | 52 |
| `precip` | Total precipitation | 0, 1, 8 | 61 |
| `snow_depth` | Snow depth | 0, 1, 11 | 66 |
| `wind_dir` | Wind direction | 0, 2, 0 | 31 |
| `wind_speed` | Wind speed | 0, 2, 1 | 32 |
| `wind_u` | Eastward wind component | 0, 2, 2 | 33 |
| `wind_v` | Northward wind component | 0, 2, 3 | 34 |
| `gust` | Wind gust speed | 0, 2, 22 | 180 |
| `pressure` | Pressure | 0, 3, 0 | 1 |
| `pressure_msl` | Pressure reduced to MSL | 0, 3, 1 | 2 |
| `geopotential_height` | Geopotential height | 0, 3, 5 | 7 |
| `cloud_cover` | Total cloud cover | 0, 6, 1 | 71 |
| `cape` | Convective available potential energy | 0, 7, 6 | 157 |
| `visibility` | Visibility | 0, 19, 0 | 20 |

A parameter outside this vocabulary falls back to a stable synthetic code key: `d{discipline}c{category}n{number}` for GRIB2, identical from either backend, such as `d0c99n5`; and `g1t{table}c{centre}p{indicator}` for a GRIB1 local parameter. GRIB itself stores only numeric codes, not author-set names, so the fallback is a code. GRIB2 identifies a parameter by its `(discipline, parameterCategory, parameterNumber)` triple, while GRIB1 identifies it by `indicatorOfParameter`. The human-readable name comes from lookup tables, including the WMO master tables and centre-local tables, not from the message. A parameter missing from those tables genuinely has no name, so the synthetic code is the accurate representation. The curated vocabulary is large enough that this fallback is rare.

### Accessing the level

After normalization, a record carries two level fields that can be used directly in templates: `level_type`, a `snake_case` token, and `level`, the numeric value in that type's canonical unit. Isobaric levels use hectopascals, while heights and depths use metres. Surface-like types such as `surface` and `mean_sea_level` have no level value. Neither field needs `this[...]`:

~~~json5
pressureLevel: {
    source: "{% if level_type == 'isobaric' %}{{ level }}{% endif %}",
    transformation: "integer",
}
~~~

The two coordinate fields can become a GeoJSON point with this mapping:

~~~json5
location: {
    type: "GeoProperty",
    source: [
        "{{ longitude }}",
        "{{ latitude }}",
    ],
    transformation: "point",
}
temperature: {
    source: "{{ temperature }}",
    transformation: "float",
}
~~~

A gridded product often stores wind as eastward and northward components rather than as speed and direction. When the two components share a grid, level, and time, Cassiopeia pivots them into columns in one record. The `wind_speed` and `wind_direction` template helpers can then derive the reported quantities directly:

~~~json5
windSpeed: {
    source: "{{ wind_speed(u=wind_u, v=wind_v) }}",
    transformation: "float",
}
windDirection: {
    source: "{{ wind_direction(u=wind_u, v=wind_v) }}",
    transformation: "integer",
}
~~~

Parameters measured at different vertical levels go into separate records because the level helps identify a grid slice. Two-metre temperature and ten-metre wind are therefore distinct records. Mapping them to the same entity identity, such as a code derived from the shared cell coordinates, merges their attributes into one observation. The [GRIB1 example](examples/18-grib1-derived-values/example.md) works through this process from beginning to end.

### Backends and features

Two compile-time features select the backend for each edition. Both link the native ecCodes library, can be toggled independently, and are enabled in the default build:

- `grib2-full` routes GRIB2 through ecCodes. ecCodes computes coordinates for projected grids and reads the full parameter tables. With this feature off, GRIB2 falls back to the pure-Rust grib-rs decoder, which has no C dependency but handles only regular latitude/longitude grids.
- `grib1` routes GRIB1 through ecCodes. With this feature off, the build reports GRIB1 as unsupported because there is no pure-Rust GRIB1 decoder.

Both backends use the same canonical vocabulary and level normalization, so changing the backend does not require a different mapping. A build made with `--no-default-features` supports only regular-grid GRIB2 through grib-rs and rejects GRIB1. Add `--features grib1` or `--features grib2-full` to re-enable either ecCodes path.

With the default build, ecCodes can decode projected GRIB2 grids, such as the Lambert conformal or polar stereographic grids used by regional models like HRRR. It handles these alongside global regular-grid products such as GFS and the ICON global model. Only the pure-Rust fallback, built with `grib2-full` off, is limited to regular latitude/longitude grids. With that fallback, a projected file fails to ingest instead of producing records. The two worked examples cover both editions: [GRIB1 regional](examples/18-grib1-derived-values/example.md) and [GRIB2 global](examples/19-grib2-byte-range/example.md).

## Choosing a format for a mapping

Start by looking at one profiled record, not at the original file alone. Then build the mapping around the fields Cassiopeia actually exposes:

1. Identify the record boundary: row, JSON object, GeoJSON feature, placemark, XML repeated child, Shapefile feature, or grid cell.
2. Locate the values in the record shape, including containers such as `properties`, `geometry`, and XML `@` or `#` fields.
3. Decide whether the source value already has the required JSON type or needs a transformation.
4. For geometry, select `GeoProperty` and use a geometry transformation that matches the source shape.
5. For KML folders or multi-layer Shapefiles, decide whether collection-specific mappings are needed.

The [mapping guide](mapping.md) explains the mapping language in detail. The format page tells you what the source record looks like; it does not replace the mapping's responsibility to choose the target entity identity and attribute model.

## Next steps

- [Write a mapping](mapping.md): build the entity from the record fields shown here.
- [Templates](templates.md): learn the expression language that a mapping's `source` uses.
- [Running Cassiopeia](running.md#choose-the-source): declare an input and its format on the command line.
