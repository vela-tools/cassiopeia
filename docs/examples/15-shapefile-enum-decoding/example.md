# Decoding shapefile enums into entity attributes

An ESRI shapefile is a bundle. `.shp` holds geometry, `.dbf` holds attributes, `.prj` describes the coordinate system, and `.cpg` may describe the encoding. The bundle usually arrives as a `.zip`. Cassiopeia reads it directly, reprojects geometry to WGS84, and exposes records in much the same way as GeoJSON and KML. This example maps the World Port Index to `Port` entities and translates coded columns into enumerated values.

## Get the data

The World Port Index (NGA Pub 150), redistributed by [HDX](https://data.humdata.org/):

```bash
wget -O world_port_index.zip "https://data.humdata.org/dataset/3f976f27-2566-4ea7-b29e-7fc6ebc90aab/resource/060abaf3-8c9f-4df4-b7e3-5335a15c6247/download/world_port_index.zip"
```

There is nothing to unzip; the zip is the input.

## The record shape

Each feature becomes a record with a `properties` object for the `.dbf` columns and a `geometry` object containing GeoJSON. The World Port Index is a point layer, and its `.prj` already declares WGS84, so the coordinates arrive in the `[longitude, latitude]` order required by an NGSI-LD `GeoProperty`. One record's columns look like this:

```text
INDEX_NO   = 61090
PORT_NAME  = SHAKOTAN
COUNTRY    = RU
HARBORTYPE = CN
HARBORSIZE = V
SHELTER    = G
TIDE_RANGE = 3.0
MED_FACIL  = Y
geometry = {
    "type": "Point",
    "coordinates": [
        146.83,
        43.87
    ]
}
```

Columns are read as `properties.<NAME>`, while the geometry passes straight through.

## The geometry, untouched

The shapefile already carries the location, so no coordinate arithmetic is needed. The `geometry` transformation passes the formed geometry into the `GeoProperty` unchanged:

```json5
location: {
    source: "{{ geometry }}",
    type: "GeoProperty",
    transformation: "geometry",
},
```

If the source used a projected coordinate system, ingest would reproject it to WGS84 first. This file is already WGS84, so its coordinates remain unchanged.

## Codes into enumerations

The index uses many short codes. The mapping turns each code into a stable, readable token with a conditional, using the technique from the [conditionals example](../04-csv-conditionals/example.md). The port's `harbourType` has nine codes:

```json5
harbourType: {
    source: "{% if properties.HARBORTYPE == 'CN' %}coastalNatural{% elif properties.HARBORTYPE == 'CB' %}coastalBreakwater{% elif properties.HARBORTYPE == 'CT' %}coastalTideGate{% elif properties.HARBORTYPE == 'LC' %}lakeOrCanal{% elif properties.HARBORTYPE == 'OR' %}openRoadstead{% elif properties.HARBORTYPE == 'RB' %}riverBasin{% elif properties.HARBORTYPE == 'RN' %}riverNatural{% elif properties.HARBORTYPE == 'RT' %}riverTideGate{% elif properties.HARBORTYPE == 'TH' %}typhoonHarbour{% endif %}",
    type: "Property",
    transformation: "string",
},
```

`harbourSize` (`V`, `S`, `M`, `L`) and `shelter` (`N`, `P`, `F`, `G`, `E`) use the same approach. Empty cells match no branch and omit the attribute, so an emitted enumerated value always belongs to the vocabulary:

| Attribute | Codes | Enumerated values |
| --- | --- | --- |
| `harbourType` | CN, CB, CT, LC, OR, RB, RN, RT, TH | coastalNatural, coastalBreakwater, coastalTideGate, lakeOrCanal, openRoadstead, riverBasin, riverNatural, riverTideGate, typhoonHarbour |
| `harbourSize` | V, S, M, L | verySmall, small, medium, large |
| `shelter` | N, P, F, G, E | none, poor, fair, good, excellent |

## A measured value and a flag

Two more columns complete the port. Tidal range is measured in metres, so it carries the `MTR` unit code (see the [units example](../14-xml-unit-code/example.md)). The medical-facility column contains `Y` when the facility is present and null otherwise; the boolean attribute appears as `true` only for the marked ports:

```json5
tidalRange: {
    source: "{{ properties.TIDE_RANGE }}",
    type: "Property",
    transformation: "float",
    properties: {
        unitCode: {
            source: "MTR",
        },
    },
},
medicalFacilities: {
    source: "{% if properties.MED_FACIL == 'Y' %}true{% endif %}",
    type: "Property",
    transformation: "boolean",
},
```

## The manifest

The zip is the input, and its format is `shapefile`. `Port` is a custom data model, the World Port Index is its own schema, matching no published one, so it is written without a schema check:

```json5
{
    version: "v1",
    inputs: [
        {
            source: "world_port_index.zip",
            mapping: "port.json5",
            format: "shapefile",
        },
    ],
    output: {
        target: "file",
        directory: "out",
        context: "none",
        validation: {
            mode: "fail-when-schema",
        },
    },
}
```

## Run it

```bash
cassiopeia map --manifest manifest.json5
```

The run writes one file, `Port.json`, with 3669 entities.

## Read the result

A port keyed by its index, located by its shape, and described with decoded values:

```json
{
    "id": "urn:ngsi-ld:Port:61090",
    "type": "Port",
    "name": {
        "type": "Property",
        "value": "SHAKOTAN"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                146.8333,
                43.8667
            ]
        }
    },
    "alpha2CountryCode": {
        "type": "Property",
        "value": "RU"
    },
    "regionNumber": {
        "type": "Property",
        "value": 61070
    },
    "harbourType": {
        "type": "Property",
        "value": "coastalNatural"
    },
    "harbourSize": {
        "type": "Property",
        "value": "verySmall"
    },
    "shelter": {
        "type": "Property",
        "value": "good"
    },
    "tidalRange": {
        "type": "Property",
        "value": 3.0,
        "unitCode": "MTR"
    },
    "medicalFacilities": {
        "type": "Property",
        "value": true
    }
}
```

Every port follows the same pattern: cryptic source codes and raw geometry become a located entity with readable attribute values.
