# Using CSV conditionals for nested attributes

Here, a semicolon-delimited bike-sharing CSV from Milan becomes `BikeHireDockingStation` entities. Tera conditionals translate source codes, a nested mapping builds the address, and Cassiopeia checks the result against the published Smart Data Model before writing it.

## Get the data

The dataset is the [BikeMi docking station locations](https://dati.comune.milano.it/dataset/ds65_infogeo_aree_sosta_bike_sharing_localizzazione_), published by the Comune di Milano on its open data portal under a Creative Commons Attribution licence. It has 325 stations, one per row.

Download it into this directory:

```bash
wget -O bikemi_stazioni.csv https://dati.comune.milano.it/dataset/cc065002-cd21-4dcb-b84f-bba2fd9e0c86/resource/4c31029c-22c9-49f7-b145-91374feac41c/download/bikemi_stazioni.csv
```

## Get the schemas

Validation checks each entity against the JSON Schema of its data model. Download the Smart Data Models catalog once; Cassiopeia stores it in a local folder and reuses it on every run:

```bash
cassiopeia sdm download
```

## The record shape

The file uses semicolons as delimiters and quotes its numeric fields. One row looks like this:

```text
id_amat;stato;numero;nome;tipo;stalli;sede;id_via;indirizzo;civico;zd_attuale;anno;LONG_X_4326;LAT_Y_4326;Location
"1";attiva;"001";Duomo;Monofacciale;"24";Carreggiata;"1";PIAZZA DEL DUOMO;;"1";"2008";9.18914146264194;45.4647459734151;"(45.4647459734151, 9.18914146264194)"
```

The mapping uses the station id (`id_amat`), status (`stato`), code (`numero`), name (`nome`), rack type (`tipo`), capacity (`stalli`), address fields (`indirizzo`, `civico`), and coordinates (`LONG_X_4326`, `LAT_Y_4326`). Cassiopeia detects the delimiter and quoting, so `stalli` becomes the number `24` rather than the text `"24"`.

## Derive values with conditionals

Two source fields need translating before they fit the model. A `source` can contain a Tera `{% if %}` block, which lets the mapping turn a code or free-text value into the model's expected value.

The model's `status` is an enumerated Property. It accepts values such as `working`, `outOfService`, and `full`, while the source contains Italian text such as `attiva`. A conditional maps the source value to the model's enum:

```json5
status: {
    source: "{% if stato == 'attiva' %}working{% else %}outOfService{% endif %}",
    type: "Property",
    transformation: "string",
},
```

The rack type has three possible codes. `description` uses `{% elif %}` and a final `{% else %}` to turn them into readable sentences:

```json5
description: {
    source: "{% if tipo == 'Monofacciale' %}Single-sided docking rack{% elif tipo == 'Bifacciale' %}Double-sided docking rack{% else %}Virtual station{% endif %}",
    type: "Property",
    transformation: "string",
},
```

## Build a nested object

An NGSI-LD Property can hold a structured object. The model's `address` is a `PostalAddress`, so the mapping uses `transformation: "object"` and a `mappings` block for the object's fields. Since only some rows have a house number, the conditional street address appends `civico` only when it is present.

```json5
address: {
    type: "Property",
    transformation: "object",
    mappings: {
        streetAddress: {
            source: "{% if civico %}{{ indirizzo }} {{ civico }}{% else %}{{ indirizzo }}{% endif %}",
            type: "Property",
            transformation: "string",
        },
        addressLocality: {
            source: "Milano",
            type: "Property",
            transformation: "string",
        },
        addressCountry: {
            source: "IT",
            type: "Property",
            transformation: "string",
        },
    },
},
```

## Write the mapping

The full mapping is in [station.json5](station.json5). Its `dataModel`, `dataModel.Transportation/BikeHireDockingStation`, identifies the published model and lets validation find its schema. The mapping also copies the name, station code, and provider as text, converts capacity to a number, and builds the location from the coordinate pair.

```json5
{
    version: "v4",
    dataModel: "dataModel.Transportation/BikeHireDockingStation",
    identity: {
        entityName: "{{ id_amat }}",
    },
    attributes: {
        name: {
            source: "{{ nome }}",
            type: "Property",
            transformation: "string",
        },
        stationCode: {
            source: "{{ numero }}",
            type: "Property",
            transformation: "string",
        },
        status: {
            // 'stato' is Italian free text ("attiva"); the model's status is a fixed enum.
            source: "{% if stato == 'attiva' %}working{% else %}outOfService{% endif %}",
            type: "Property",
            transformation: "string",
        },
        description: {
            // Turn the physical rack type into a readable sentence.
            source: "{% if tipo == 'Monofacciale' %}Single-sided docking rack{% elif tipo == 'Bifacciale' %}Double-sided docking rack{% else %}Virtual station{% endif %}",
            type: "Property",
            transformation: "string",
        },
        totalSlotNumber: {
            source: "{{ stalli }}",
            type: "Property",
            transformation: "integer",
        },
        dataProvider: {
            source: "Comune di Milano",
            type: "Property",
            transformation: "string",
        },
        location: {
            // GeoJSON coordinates are ordered longitude, then latitude.
            source: [
                "{{ LONG_X_4326 }}",
                "{{ LAT_Y_4326 }}",
            ],
            type: "GeoProperty",
            transformation: "point",
        },
        address: {
            type: "Property",
            transformation: "object",
            mappings: {
                streetAddress: {
                    // The house number is present on only some rows; append it when it is.
                    source: "{% if civico %}{{ indirizzo }} {{ civico }}{% else %}{{ indirizzo }}{% endif %}",
                    type: "Property",
                    transformation: "string",
                },
                addressLocality: {
                    source: "Milano",
                    type: "Property",
                    transformation: "string",
                },
                addressCountry: {
                    source: "IT",
                    type: "Property",
                    transformation: "string",
                },
            },
        },
    },
}
```

## Run it

Run the mapping from this directory:

```bash
cassiopeia map \
    --input bikemi_stazioni.csv \
    --mapping station.json5 \
    --type csv \
    --output out \
    --context none \
    --validation-mode fail \
    --validation-representation simplified
```

`--validation-mode fail` stops the run if any entity fails its schema. `--validation-representation simplified` checks the key-values form described by Smart Data Model schemas, so `status` is validated as the string `"working"`, not as its normalized Property wrapper. Cassiopeia finds the schema for `dataModel.Transportation/BikeHireDockingStation` in the folder populated by `cassiopeia sdm download`.

The command creates `out/BikeHireDockingStation.json`, a single JSON array of 325 `BikeHireDockingStation` entities.

## Read the result

The default output is normalized, so each attribute carries its NGSI-LD type. Station `034`, whose row has a house number, comes out like this:

```json
{
    "id": "urn:ngsi-ld:BikeHireDockingStation:38",
    "type": "BikeHireDockingStation",
    "name": {
        "type": "Property",
        "value": "Cairoli"
    },
    "stationCode": {
        "type": "Property",
        "value": "034"
    },
    "status": {
        "type": "Property",
        "value": "working"
    },
    "description": {
        "type": "Property",
        "value": "Single-sided docking rack"
    },
    "totalSlotNumber": {
        "type": "Property",
        "value": 21
    },
    "dataProvider": {
        "type": "Property",
        "value": "Comune di Milano"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                9.18215375148315,
                45.4676530174474
            ]
        }
    },
    "address": {
        "type": "Property",
        "value": {
            "streetAddress": "VIA SAN GIOVANNI SUL MURO 1",
            "addressLocality": "Milano",
            "addressCountry": "IT"
        }
    }
}
```

The conditionals have done their work: `status` is the enum value `working`, `description` is the sentence for a single-sided rack, and `streetAddress` carries the house number `1` because this row supplied one. Every one of these entities passed the `BikeHireDockingStation` schema.
