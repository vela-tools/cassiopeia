# Mapping GeoJSON to a Smart Data Model

The first two examples used their own entity types, `ChemicalElement` and `City`. This one turns GeoJSON features into `OffStreetParking` entities from a published Smart Data Model and checks each entity against that model's schema. Because the features already contain geometry, the mapping can pass the locations through unchanged.

The data describes disabled-parking locations in Vienna. The `OffStreetParking` model was designed with larger facilities in mind, but its attributes also fit individual spaces. The example focuses on choosing and using that published model.

## Get the data

The dataset is [Behindertenparkplätze Standorte Wien](https://www.data.gv.at/katalog/en/dataset/4315e096-f51e-4b56-8235-57be9789a62c) (disabled-parking locations in Vienna), published by the City of Vienna and licensed under Creative Commons Attribution 4.0. It has 3,854 features, one per parking location.

Download it as GeoJSON into this directory:

```bash
wget -O parking.json "https://data.wien.gv.at/daten/geo?service=WFS&request=GetFeature&version=1.1.0&typeName=ogdwien:BEHINDERTENPARKPLATZOGD&srsName=EPSG:4326&outputFormat=json"
```

## Get the schemas

Validation checks each entity against the JSON Schema of its data model. Download the Smart Data Models catalog once; Cassiopeia stores it in a local folder and reuses it on every run:

```bash
cassiopeia sdm download
```

## The record shape

A GeoJSON `FeatureCollection` is a list of features. Cassiopeia exposes each feature as a record with an `id`, `geometry`, and `properties` object. One record looks like this:

```json
{
    "type": "Feature",
    "id": "BEHINDERTENPARKPLATZOGD.15652565",
    "geometry": {
        "type": "Point",
        "coordinates": [
            16.38946141,
            48.19718606
        ]
    },
    "properties": {
        "STRNAM": "Barichgasse",
        "STELLPL_ANZ": 1,
        "BEZIRK": 3,
        "KATEGORIE": 3
    }
}
```

The mapping accesses those values with `{{ id }}`, `{{ geometry }}`, and `{{ properties.STRNAM }}`.

## Choose the model

Examples 1 and 2 used bare model names. This mapping names a published model together with its repository:

```json5
dataModel: "dataModel.Parking/OffStreetParking",
```

The `dataModel.Parking` qualifier tells Cassiopeia where to find `OffStreetParking` in the downloaded catalog, including its schema. The model defines the available attributes and accepted shapes: vocabulary arrays for `category` and `allowedVehicleType`, text for `openingHours`, `description`, and `dataProvider`, an `address` object, and a required `location`. The mapping supplies the fields supported by the source.

## Write the mapping

The full mapping is in [parking.json5](parking.json5). It maps the street and house number, description, opening hours, and coordinates directly, without conditional logic.

```json5
{
    version: "v4",
    dataModel: "dataModel.Parking/OffStreetParking",
    identity: {
        entityName: "{{ id }}",
    },
    attributes: {
        name: {
            source: "{{ properties.STRNAM }}",
            type: "Property",
            transformation: "string",
        },
        description: {
            source: "{{ properties.BESCHREIBUNG }}",
            type: "Property",
            transformation: "string",
        },
        category: {
            source: "forDisabled",
            type: "Property",
            transformation: "array",
        },
        allowedVehicleType: {
            source: "car",
            type: "Property",
            transformation: "array",
        },
        openingHours: {
            source: "{{ properties.ZEITRAUM }}",
            type: "Property",
            transformation: "string",
        },
        dataProvider: {
            source: "Stadt Wien",
            type: "Property",
            transformation: "string",
        },
        source: {
            source: "https://www.data.gv.at/katalog/en/dataset/4315e096-f51e-4b56-8235-57be9789a62c",
            type: "Property",
            transformation: "string",
        },
        location: {
            // The feature already contains the geometry, so no coordinate mapping is needed.
            source: "{{ geometry }}",
            type: "GeoProperty",
            transformation: "geometry",
        },
        address: {
            type: "Property",
            transformation: "object",
            mappings: {
                streetAddress: {
                    source: "{{ properties.STRNAM }} {{ properties.ONR_VON }}",
                    type: "Property",
                    transformation: "string",
                },
                addressLocality: {
                    source: "Wien",
                    type: "Property",
                    transformation: "string",
                },
                addressCountry: {
                    source: "Austria",
                    type: "Property",
                    transformation: "string",
                },
            },
        },
    },
}
```

The feature's own `id` becomes the entity identity. `name`, `description`, and `openingHours` copy their source fields. `category` and `allowedVehicleType` use the model's vocabulary values, `forDisabled` and `car`, and `transformation: "array"` gives each the required array shape. `dataProvider` and `source` record provenance. The `address` mapping joins the street and house number into `streetAddress`. Empty descriptions or opening hours are omitted rather than written as null.

In example 2, the mapping built a point from two coordinate strings. Here the feature already has a geometry, so the `geometry` transformation passes it directly into the `GeoProperty`.

Two source fields are deliberately left for a [later example](../05-geojson-attribute-guards/example.md), which returns to this same parking dataset. They need conditional logic to pass validation: the numeric `KATEGORIE` must become the model's `category` and `requiredPermit` vocabulary values, while `STELLPL_ANZ` uses `-1` for an unknown count, which `totalSpotNumber` rejects.

## Run it

Run the mapping from this directory:

```bash
cassiopeia map \
    --input parking.json \
    --mapping parking.json5 \
    --type geojson \
    --output out \
    --context none \
    --validation-mode fail \
    --validation-representation simplified
```

`--validation-mode fail` aborts the run if any entity does not match the `OffStreetParking` schema, so a clean run proves all 3,854 entities conform. `--validation-representation simplified` validates the key-values form described by Smart Data Model schemas. The command creates `out/OffStreetParking.json`.

## Read the result

A row that fills in the optional fields comes out like this:

```json
{
    "id": "urn:ngsi-ld:OffStreetParking:BEHINDERTENPARKPLATZOGD15651436",
    "type": "OffStreetParking",
    "name": {
        "type": "Property",
        "value": "Prager Straße"
    },
    "description": {
        "type": "Property",
        "value": "Bezirksmuseum, Pensionistenclub"
    },
    "category": {
        "type": "Property",
        "value": [
            "forDisabled"
        ]
    },
    "allowedVehicleType": {
        "type": "Property",
        "value": [
            "car"
        ]
    },
    "openingHours": {
        "type": "Property",
        "value": "v. 8-18h"
    },
    "dataProvider": {
        "type": "Property",
        "value": "Stadt Wien"
    },
    "source": {
        "type": "Property",
        "value": "https://www.data.gv.at/katalog/en/dataset/4315e096-f51e-4b56-8235-57be9789a62c"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                16.39428003,
                48.26335834
            ]
        }
    },
    "address": {
        "type": "Property",
        "value": {
            "streetAddress": "Prager Straße 33",
            "addressLocality": "Wien",
            "addressCountry": "Austria"
        }
    }
}
```

The entity is an `OffStreetParking` from the published model, and it passed that model's schema. The id keeps only URN-safe characters, so the dot in the feature id is dropped.
