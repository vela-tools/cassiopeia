# Building a CSV relationship graph across five sources

The [airport and airline example](../09-csv-manifest/example.md) becomes a connected graph here. Five OpenFlights files provide countries, aircraft models, airports, airlines, and routes. Routes point to an airline, two airports, and an aircraft model; airports and airlines point to countries. Four types use published Smart Data Models, while `Country` is a custom relationship target. One manifest builds the graph.

## Get the data

All five files come from [OpenFlights](https://github.com/jpatokal/openflights), under the Open Database License (ODbL):

```bash
wget -O countries.dat https://raw.githubusercontent.com/jpatokal/openflights/master/data/countries.dat
wget -O planes.dat https://raw.githubusercontent.com/jpatokal/openflights/master/data/planes.dat
wget -O airports.dat https://raw.githubusercontent.com/jpatokal/openflights/master/data/airports.dat
wget -O airlines.dat https://raw.githubusercontent.com/jpatokal/openflights/master/data/airlines.dat
wget -O routes.dat https://raw.githubusercontent.com/jpatokal/openflights/master/data/routes.dat
```

## Get the schemas

Four of the models are published Smart Data Models. Download the catalog once:

```bash
cassiopeia sdm download
```

## The graph

The source rows already contain the IDs or names needed for these links:

```text
Country  ←  Airport  ←  Route  →  Airline  →  Country
                ↑         │
                └─────────┤
                          ↓
                    AircraftModel
```

A route names its airline, departure and arrival airports, and equipment. An airport and an airline each name their country.

## Two ways to key a relationship

As in the [previous relationship example](../10-json-relationships/example.md), each relationship object URN combines a source value with the target model. This graph uses two kinds of source value.

Most links use an **id**. A route row carries OpenFlights IDs for its airline and airports, so those relationships read the value directly:

```json5
belongsToAirline: {
    source: "{{ this[1] }}",
    type: "Relationship",
    target: {
        entity: "Airline",
    },
},
departsFromAirport: {
    source: "{{ this[3] }}",
    type: "Relationship",
    target: {
        entity: "Airport",
    },
},
```

The airport-to-country link uses a **name** because the airport file stores the country as text rather than as an ID. The country mapping uses that name as its identity, so both sides produce the same URN:

```json5
// in country.json5
identity: {
    entityName: "{{ this[0] }}",
},
// in airport.json5
inCountry: {
    source: "{{ this[3] }}",
    type: "Relationship",
    target: {
        entity: "Country",
    },
},
```

An airport in the United Kingdom points to `urn:ngsi-ld:Country:UnitedKingdom`, the ID produced for the United Kingdom row. A few names differ between the two files; those links are valid URNs, but no country entity matches them.

## The route mapping

A route has no source ID, so its identity combines the airline and its two endpoints. Each triple is distinct, making each route its own `Flight`. The equipment column can list several aircraft codes, but the Flight model defines `hasAircraftModel` as one relationship, so this mapping keeps the first code. Text values are split; an all-numeric code is already read as a number and is used as-is:

```json5
{
    version: "v4",
    dataModel: "dataModel.Aeronautics/Flight",
    identity: {
        entityName: "{{ this[0] }}-{{ this[2] }}-{{ this[4] }}",
    },
    attributes: {
        belongsToAirline: {
            source: "{{ this[1] }}",
            type: "Relationship",
            target: {
                entity: "Airline",
            },
        },
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
        hasAircraftModel: {
            source: "{% if this[8] is string %}{{ this[8] | split(pat=' ') | first }}{% else %}{{ this[8] }}{% endif %}",
            type: "Relationship",
            target: {
                entity: "AircraftModel",
            },
        },
    },
}
```

The Flight model defines all four relationships, so the resulting entity validates against its schema.

## The country, without a schema

The country list is the only source without a published model, so `country.json5` uses a plain `Country` type. The other four types have Smart Data Model schemas. In `fail-when-schema` mode, Cassiopeia validates those four types and writes `Country` without a schema check.

## The manifest

```json5
{
    version: "v1",
    inputs: [
        {
            source: "countries.dat",
            mapping: "country.json5",
            format: "csv",
        },
        {
            source: "planes.dat",
            mapping: "plane.json5",
            format: "csv",
        },
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
        {
            source: "routes.dat",
            mapping: "route.json5",
            format: "csv",
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

The run writes `Country.json`, `AircraftModel.json`, `Airport.json`, `Airline.json`, and `Flight.json`. The airport and airline counts match the previous example, and all 67,663 routes become `Flight` entities.

## Read the result

A `Flight` is a junction entity: one ID and four relationships to the other types.

```json
{
    "id": "urn:ngsi-ld:Flight:LH-EDI-FRA",
    "type": "Flight",
    "belongsToAirline": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Airline:3320",
        "objectType": "Airline"
    },
    "departsFromAirport": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Airport:535",
        "objectType": "Airport"
    },
    "arrivesToAirport": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Airport:340",
        "objectType": "Airport"
    },
    "hasAircraftModel": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:AircraftModel:321",
        "objectType": "AircraftModel"
    }
}
```

London Heathrow points at its country, which is a real entity in the same run:

```json
{
    "id": "urn:ngsi-ld:Airport:507",
    "type": "Airport",
    "name": {
        "type": "Property",
        "value": "London Heathrow Airport"
    },
    "codeIATA": {
        "type": "Property",
        "value": "LHR"
    },
    "inCountry": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Country:UnitedKingdom",
        "objectType": "Country"
    }
}
```

```json
{
    "id": "urn:ngsi-ld:Country:UnitedKingdom",
    "type": "Country",
    "name": {
        "type": "Property",
        "value": "United Kingdom"
    },
    "isoCode": {
        "type": "Property",
        "value": "GB"
    },
    "dafifCode": {
        "type": "Property",
        "value": "UK"
    }
}
```

This mapping stays within the Flight Smart Data Model by keeping only a route's first aircraft, because `hasAircraftModel` is a single relationship. Routes often list several aircraft. The [next example](../12-csv-list-relationship/example.md) keeps them all with a newer NGSI-LD type that the published schema does not describe.

## Carrying every aircraft, on-spec

Reducing the equipment to its primary aircraft is a modelling choice, not the only one. To carry _all_ of a route's aircraft while still validating against the Flight model, you would keep `hasAircraftModel` as a `Relationship` but give it several instances, one per aircraft, each an object distinguished by its own `datasetId` (ETSI GS CIM 009 v1.9.1 clause 4.5.5). A _multi-attribute_ is one attribute name holding several instances, serialized as a JSON array; at most one instance may omit its `datasetId`, so linking to more than one aircraft means every additional instance names its own. Each link stays a plain `Relationship` the schema recognises, which is the difference from the [`ListRelationship`](../12-csv-list-relationship/example.md) the next example reaches for: that gathers the aircraft into one ordered `objectList` the Flight model does not describe, whereas the multi-attribute keeps the shape the model already expects. The [datasetId example](../21-json-dataset-id/example.md) works the same multi-instance mechanism on a Property.
