# Mapping KMZ folder namespacing into entities

KML stores placemarks as XML. KMZ packages the same content in a zip archive, usually with one `.kml` file. Cassiopeia reads both formats and exposes each placemark as a record with attributes and geometry. This example maps Esri's World Cities layer to `City` entities and shows how the folder name becomes part of the record path.

## Get the data

Esri publishes World Cities as KML. The commands below download it and package it as a `.kmz`:

```bash
wget -O world-cities.kml "https://opendata.arcgis.com/datasets/esri::world-cities.kml"
zip world-cities.kmz world-cities.kml && rm world-cities.kml
```

The `.kmz` is the input; there is nothing to unzip by hand.

## The record shape

A KML placemark stores its attributes in `<ExtendedData>` and its position in a geometry. Esri writes the attributes as a typed `<SchemaData>` table. One city's placemark looks like this:

```xml
<Placemark id="World_Cities.1460">
  <ExtendedData><SchemaData schemaUrl="#World_Cities">
    <SimpleData name="FID">1460</SimpleData>
    <SimpleData name="CITY_NAME">Tokyo</SimpleData>
    <SimpleData name="CNTRY_NAME">Japan</SimpleData>
    <SimpleData name="ADMIN_NAME">Kanto</SimpleData>
    <SimpleData name="STATUS">National and provincial capital</SimpleData>
    <SimpleData name="POP">9272740</SimpleData>
  </SchemaData></ExtendedData>
  <Point><coordinates>139.809,35.683</coordinates></Point>
</Placemark>
```

Each `<SimpleData name="X">` becomes a property `X`, and the `<Point>` becomes the geometry. The file is wrapped in one `<Folder>` named `World_Cities`. Cassiopeia treats folders as collections and nests each record under a lower-case, underscored version of the folder name. Here the placemark data is under `world_cities` rather than at the top level:

```text
world_cities.properties.CITY_NAME   -> "Tokyo"
world_cities.properties.POP         -> "9272740"   (SchemaData values are strings)
world_cities.geometry               -> {
    "type": "Point",
    "coordinates": [
        139.809,
        35.683
    ]
}
```

The [multi-folder example](../17-kml-folder-collections/example.md) uses the same namespacing to keep several folders apart. This file has one folder, so every record uses the `world_cities` prefix.

## The geometry, untouched

The placemark already carries a WGS84 point, so no coordinate arithmetic is needed. The `geometry` transformation passes it through as a `GeoProperty`:

```json5
location: {
    source: "{{ world_cities.geometry }}",
    type: "GeoProperty",
    transformation: "geometry",
},
```

## A string turned into a number, a phrase into an enumeration

Everything in `<SchemaData>` arrives as text, so the mapping parses the population before creating a number:

```json5
population: {
    source: "{{ world_cities.properties.POP | int }}",
    type: "Property",
    transformation: "integer",
},
```

The `STATUS` column records a city's administrative role as one of six phrases. A conditional converts each phrase into a stable token, using the technique from the [conditionals example](../04-csv-conditionals/example.md):

| `STATUS` | `administrativeRole` |
| --- | --- |
| National capital | nationalCapital |
| Provincial capital | provincialCapital |
| National and provincial capital | nationalAndProvincialCapital |
| National capital and provincial capital enclave | nationalCapitalProvincialEnclave |
| Provincial capital enclave | provincialCapitalEnclave |
| Other | other |

## The manifest

The input is the `.kmz`, so its format is `kmz`. `City` is a custom type with no published schema, so the output is not schema-checked:

```json5
{
    version: "v1",
    inputs: [
        {
            source: "world-cities.kmz",
            mapping: "city.json5",
            format: "kmz",
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

The run writes one file, `City.json`, with 2540 entities.

## Read the result

Tokyo, located by its placemark and described by decoded columns:

```json
{
    "id": "urn:ngsi-ld:City:1460",
    "type": "City",
    "name": {
        "type": "Property",
        "value": "Tokyo"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                139.809,
                35.683
            ]
        }
    },
    "country": {
        "type": "Property",
        "value": "Japan"
    },
    "adminRegion": {
        "type": "Property",
        "value": "Kanto"
    },
    "population": {
        "type": "Property",
        "value": 9272740
    },
    "administrativeRole": {
        "type": "Property",
        "value": "nationalAndProvincialCapital"
    }
}
```

KMZ produces the same records as the KML. The archive only changes how the file is packaged.
