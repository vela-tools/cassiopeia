# Combining CSV sources with a manifest

This example reads two unrelated OpenFlights CSV files and writes two entity types. Airport rows become `Airport` entities and airline rows become `Airline` entities, each checked against its published Smart Data Model. A manifest connects each source to its mapping and stores the shared output settings. Neither file has a header row, so the mappings address columns by position.

## Get the data

Both datasets come from [OpenFlights](https://github.com/jpatokal/openflights), which publishes them under the Open Database License (ODbL). The airports file has 7698 rows, one per airport; the airlines file has 6162, one per airline.

Download both into this directory:

```bash
wget -O airports.dat https://raw.githubusercontent.com/jpatokal/openflights/master/data/airports.dat
wget -O airlines.dat https://raw.githubusercontent.com/jpatokal/openflights/master/data/airlines.dat
```

## Get the schemas

Both models are published Smart Data Models, so validation checks each entity against the model's schema. Download the catalog once; Cassiopeia stores it locally and reuses it on every run:

```bash
cassiopeia sdm download
```

## The record shape

Neither file has a header row; the first row is already data. An airport row and the first airline rows look like this:

```text
1,"Goroka Airport","Goroka","Papua New Guinea","GKA","AYGA",-6.081689834590001,145.391998291,5282,10,"U","Pacific/Port_Moresby","airport","OurAirports"
```

```text
-1,"Unknown",\N,"-","N/A",\N,\N,"Y"
2,"135 Airways",\N,"","GNL","GENERAL","United States","N"
3,"1Time Airline",\N,"1T","RNX","NEXTIME","South Africa","Y"
```

The airport columns, in order, are: id, name, city, country, IATA code, ICAO code, latitude, longitude, altitude, timezone, DST, tz name, type, and source. The airline columns are: id, name, alias, IATA code, ICAO code, callsign, country, and active flag. Missing values are written as `\N`, the null token these SQL-style dumps use; Cassiopeia reads `\N` as an absent value, not as the literal text.

## Address columns by position

A mapping normally reads a source field by name, such as `{{ city }}`. A headerless file has no field names, so the mapping uses zero-based positions: `this[0]` is the first column, `this[1]` the second, and so on. Cassiopeia recognizes that the first row is data and keeps it as a record.

The airport identity and name are the first two columns:

```json5
identity: {
    entityName: "{{ this[0] }}",
},
attributes: {
    name: {
        source: "{{ this[1] }}",
        type: "Property",
        transformation: "string",
    },
    // ...
}
```

## Keep only well-formed codes

The IATA and ICAO codes are optional, but present values must have a fixed shape. The schema expects three uppercase letters for `codeIATA` and four for `codeICAO`. Since the source contains blanks, `\N`, `-`, `N/A`, and real codes, each mapping uses `matching` to keep only values that fit.

```json5
codeIATA: {
    source: "{% if this[4] is string and this[4] is matching(pat='^[A-Z]{3}$') %}{{ this[4] }}{% endif %}",
    type: "Property",
    transformation: "string",
},
```

The `is string` check comes first because the CSV reader may parse a numeric-looking field as a number, while `matching` expects text. If the check fails, the source produces nothing and the attribute is omitted instead of written with an invalid value.

## Write the mappings

Each source has its own mapping. [airport.json5](airport.json5) builds the name, two codes, location, and nested address:

```json5
{
    version: "v4",
    dataModel: "dataModel.Aeronautics/Airport",
    identity: {
        entityName: "{{ this[0] }}",
    },
    attributes: {
        name: {
            source: "{{ this[1] }}",
            type: "Property",
            transformation: "string",
        },
        codeIATA: {
            source: "{% if this[4] is string and this[4] is matching(pat='^[A-Z]{3}$') %}{{ this[4] }}{% endif %}",
            type: "Property",
            transformation: "string",
        },
        codeICAO: {
            source: "{% if this[5] is string and this[5] is matching(pat='^[A-Z]{4}$') %}{{ this[5] }}{% endif %}",
            type: "Property",
            transformation: "string",
        },
        location: {
            // Columns 6 and 7 are latitude and longitude; GeoJSON orders a point longitude first.
            source: [
                "{{ this[7] }}",
                "{{ this[6] }}",
            ],
            type: "GeoProperty",
            transformation: "point",
        },
        address: {
            type: "Property",
            transformation: "object",
            mappings: {
                addressLocality: {
                    source: "{{ this[2] }}",
                    type: "Property",
                    transformation: "string",
                },
                addressCountry: {
                    source: "{{ this[3] }}",
                    type: "Property",
                    transformation: "string",
                },
            },
        },
    },
}
```

[airline.json5](airline.json5) is smaller. It maps the name, two codes with their airline-specific patterns, and the callsign, which only needs a guard for missing values:

```json5
{
    version: "v4",
    dataModel: "dataModel.Aeronautics/Airline",
    identity: {
        entityName: "{{ this[0] }}",
    },
    attributes: {
        name: {
            source: "{{ this[1] }}",
            type: "Property",
            transformation: "string",
        },
        codeIATA: {
            source: "{% if this[3] is string and this[3] is matching(pat='^[A-Z0-9]{2}$') %}{{ this[3] }}{% endif %}",
            type: "Property",
            transformation: "string",
        },
        codeICAO: {
            source: "{% if this[4] is string and this[4] is matching(pat='^[A-Z]{3}$') %}{{ this[4] }}{% endif %}",
            type: "Property",
            transformation: "string",
        },
        callSign: {
            source: "{% if this[5] %}{{ this[5] }}{% endif %}",
            type: "Property",
            transformation: "string",
        },
    },
}
```

## Write the manifest

A manifest describes the sources, mappings, and output for the whole run, so the command needs no inline options. The full manifest is in [manifest.json5](manifest.json5):

```json5
{
    version: "v1",
    inputs: [
        {
            source: "airports.dat",
            mapping: "airport.json5",
            format: "csv",
        },
        {
            source: "airlines.dat",
            mapping: "airline.json5",
            format: "csv",
        },
    ],
    output: {
        target: "file",
        directory: "out",
        context: "none",
        validation: {
            mode: "fail",
        },
    },
}
```

Each `inputs` entry binds one source to one mapping. The inputs use different columns, models, and output types but share the `output` block. Setting `validation.mode` to `"fail"` checks every entity against its model's schema and stops the run on the first violation. The manifest uses version `v1`; mapping documents use version `v4`.

## Run it

The manifest supplies everything, so the command only names the manifest:

```bash
cassiopeia map --manifest manifest.json5
```

Cassiopeia reads both sources, applies the matching mapping, validates the entities, and writes one file per type. The result is `out/Airport.json` with 7698 entities and `out/Airline.json` with 6162 entities.

## Read the result

The output is normalized, so each attribute carries its NGSI-LD type. The first airport row, Goroka Airport, has both codes, a location, and an address:

```json
{
    "id": "urn:ngsi-ld:Airport:1",
    "type": "Airport",
    "name": {
        "type": "Property",
        "value": "Goroka Airport"
    },
    "codeIATA": {
        "type": "Property",
        "value": "GKA"
    },
    "codeICAO": {
        "type": "Property",
        "value": "AYGA"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                145.391998291,
                -6.081689834590001
            ]
        }
    },
    "address": {
        "type": "Property",
        "value": {
            "addressLocality": "Goroka",
            "addressCountry": "Papua New Guinea"
        }
    }
}
```

American Airlines, from the second source, is written to the airline file with its two codes and callsign:

```json
{
    "id": "urn:ngsi-ld:Airline:24",
    "type": "Airline",
    "name": {
        "type": "Property",
        "value": "American Airlines"
    },
    "codeIATA": {
        "type": "Property",
        "value": "AA"
    },
    "codeICAO": {
        "type": "Property",
        "value": "AAL"
    },
    "callSign": {
        "type": "Property",
        "value": "AMERICAN"
    }
}
```

Blank, `\N`, and malformed codes are omitted. The guards keep every emitted code within the schema's required shape, so all 13,860 entities pass validation.
