# Mapping CSV multi-attribute relationships across airport roles

The [relationship graph example](../11-csv-relationship-graph/example.md) gives a `Flight` two airport attributes: `departsFromAirport` and `arrivesToAirport`. Both link a flight to an airport, but they describe different roles. This example puts those links under one `servesAirport` attribute and uses `datasetId` to keep the departure and arrival instances separate.

It reuses the five OpenFlights sources from example 11. Only the route mapping changes. The other four mappings and the source files stay the same.

## Choosing the relationship type

A single `Relationship` points to one target entity. `belongsToAirline` uses this form because each flight has one airline.

A multi-attribute `Relationship` uses one attribute name for several Relationship instances. Each instance has its own `object` and `datasetId`, and the entity serializes them as an array. This fits a fixed group of links that have the same meaning but different roles, such as a departure airport and an arrival airport.

A `ListRelationship` stores several targets as one ordered `objectList`. It fits a variable-length collection of equivalent links, such as the aircraft codes listed on a route. The [list relationship example](../12-csv-list-relationship/example.md) shows that form.

Here, a multi-attribute Relationship is the better fit. There are two airport roles, and each role needs its own target. The aircraft list remains a `ListRelationship` because its length varies and its entries do not have separate roles.

## Get the data

The five files are the same OpenFlights sources used by example 11. OpenFlights publishes them under the Open Database License (ODbL):

```bash
wget -O countries.dat https://raw.githubusercontent.com/jpatokal/openflights/master/data/countries.dat
wget -O planes.dat https://raw.githubusercontent.com/jpatokal/openflights/master/data/planes.dat
wget -O airports.dat https://raw.githubusercontent.com/jpatokal/openflights/master/data/airports.dat
wget -O airlines.dat https://raw.githubusercontent.com/jpatokal/openflights/master/data/airlines.dat
wget -O routes.dat https://raw.githubusercontent.com/jpatokal/openflights/master/data/routes.dat
```

## Get the schemas

Four of the five models are published Smart Data Models. Download the catalog once:

```bash
cassiopeia sdm download
```

## Change the route mapping

Only `route.json5` differs from example 11. The old mapping used separate airport attributes:

```json5
departsFromAirport: {
    source: "{{ this[3] }}",
    type: "Relationship",
    target: {
        entity: "Airport",
    },
},
arrivesToAirport: {
    source: "{{ this[5] }}",
    type: "Relationship",
    target: {
        entity: "Airport",
    },
},
```

The new mapping puts both links in `servesAirport`. Each entry in `instances` reads a different source column and supplies a different dataset identifier:

```json5
servesAirport: {
    type: "Relationship",
    target: {
        entity: "Airport",
    },
    instances: [
        {
            source: "{{ this[3] }}",
            properties: {
                datasetId: {
                    source: "urn:ngsi-ld:dataset:role:departure",
                },
            },
        },
        {
            source: "{{ this[5] }}",
            properties: {
                datasetId: {
                    source: "urn:ngsi-ld:dataset:role:arrival",
                },
            },
        },
    ],
},
```

`belongsToAirline` and `hasAircraftModel` stay single relationships, so the mapping shows both the single-instance and multi-attribute forms together.

## How `instances` works

An attribute with `instances` produces one attribute instance for each entry. The entries share the attribute's `type` and `target`, but each one has its own source and qualifiers. Here, both entries target `Airport`; the first reads column 3 and uses the departure dataset ID, while the second reads column 5 and uses the arrival dataset ID.

Each airport ID combines with the shared target model to produce an `Airport` URN, as it does for a single Relationship. If one source ID is missing, that instance produces no link and the other instance remains.

The [datasetId example](../21-json-dataset-id/example.md) uses the same mapping pattern for several forecast values under one Property name. Here, the pattern separates two roles of one Relationship.

## Why validation still uses `fail-when-schema`

The manifest uses `mode: "fail-when-schema"`. Cassiopeia validates the four Aeronautics types against their published schemas and writes the schemaless `Country` entities without validation.

The cached `dataModel.Aeronautics/Flight` schema requires `id` and `type`, but it does not set `additionalProperties: false`. It therefore accepts the new `servesAirport` attribute because the mapping adds a new attribute rather than changing one defined by the model. This differs from the [ListRelationship example](../12-csv-list-relationship/example.md), where changing `hasAircraftModel` from a Relationship to a ListRelationship no longer matches the published schema.

## Run it

```bash
cassiopeia map --manifest manifest.json5
```

The run writes `Country.json`, `AircraftModel.json`, `Airport.json`, `Airline.json`, and `Flight.json`, the same five files as example 11. The `Flight` entities now carry both airport links under `servesAirport`.

## Read the result

The normalized entity contains an array of Relationship instances under `servesAirport`. Each instance has its own `datasetId`:

```json
{
    "id": "urn:ngsi-ld:Flight:LH-EDI-FRA",
    "type": "Flight",
    "belongsToAirline": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Airline:3320",
        "objectType": "Airline"
    },
    "servesAirport": [
        {
            "type": "Relationship",
            "object": "urn:ngsi-ld:Airport:535",
            "objectType": "Airport",
            "datasetId": "urn:ngsi-ld:dataset:role:departure"
        },
        {
            "type": "Relationship",
            "object": "urn:ngsi-ld:Airport:340",
            "objectType": "Airport",
            "datasetId": "urn:ngsi-ld:dataset:role:arrival"
        }
    ],
    "hasAircraftModel": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:AircraftModel:321",
        "objectType": "AircraftModel"
    }
}
```

`belongsToAirline` and `hasAircraftModel` are single Relationship objects. `servesAirport` is an array because it has two instances of the same attribute, and the dataset IDs identify their roles.

## Which form should you use?

Use a single Relationship for one link. Use a multi-attribute Relationship when several links share one attribute name but need separate roles or datasets. Use a `ListRelationship` when the source contains a variable-length ordered collection of equivalent links.
