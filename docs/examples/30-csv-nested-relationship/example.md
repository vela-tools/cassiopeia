# Mapping CSV nested relationships in movie credits

Most relationship examples stop at the target entity. A `Flight` points to an `Airport`, or a `Route` points to an `AircraftModel`, and the link ends there. This example adds sub-attributes to a relationship: a `Movie` points to its lead `Person`, and that relationship carries another link to the `Character` the actor plays.

The source is the TMDB 5000 credits dataset. Each film stores its `cast` as an array of objects with an `id`, a `character`, and a billing `order`. The CSV reader keeps that array as a string, so the mapping decodes it before reading the first member or building the full cast list.

## Relationships can have sub-attributes

NGSI-LD allows a relationship to carry its own sub-attributes. In a mapping, entries under `properties` normally become nested Properties. They can also declare another supported attribute type, including `Relationship` (ETSI GS CIM 009 v1.9.1 clauses 4.5.2.2 and 4.5.3). The same rule applies again inside the nested attribute.

`hasLeadActor` points to a `Person` and has two sub-attributes:

- `playsCharacter` is a nested `Relationship` that points to the `Character` played by the actor.
- `billingOrder` is an ordinary `Property` containing the actor's billing position.

`hasCast` uses a `ListRelationship` to point to every cast member. It also carries a `castSize` Property, which records the size of that list.

These three shapes are different. A flat `Relationship` has one target and no sub-attributes, as in the [relationship graph example](../11-csv-relationship-graph/example.md). A multi-attribute `Relationship` has several `datasetId`-tagged instances under one attribute name, as in the [multi-attribute relationship example](../29-csv-multi-attribute-relationship/example.md). A nested `Relationship` has one target whose relationship object contains another attribute, which is the pattern used here.

## Get the data

The example uses the TMDB credits CSV. Its `cast` column contains a JSON array encoded as text. Download the file:

```bash
wget -O tmdb_5000_credits.csv https://raw.githubusercontent.com/andandandand/CSV-datasets/master/tmdb_5000_credits.csv
```

Each row's `cast` cell is a JSON array stored as text. That is the input shape expected by the mapping's `json_decode` filter, which parses the cell before the template reads the first member or loops through the list.

## Decode the cast field

The template receives `cast` as JSON text. Each member has an `id`, a `character`, and an `order`; `json_decode` turns the text into a structured value that the template can index and loop over.

To read the first actor's id, use this expression:

```json5
source: "{{ cast | json_decode | first | get(key='id') }}"
```

`json_decode | first` selects the first cast member. `get(key='id')` then reads its id. A `{% for member in cast | json_decode %}` loop can walk through every member.

## Add a relationship to a relationship

`hasLeadActor` reads the first cast member's id for its own `object`. Its `properties` block defines the nested attributes. `playsCharacter` is a complete Relationship declaration with its own `type`, `target`, and `source`:

```json5
hasLeadActor: {
    type: "Relationship",
    target: {
        entity: "Person",
    },
    source: "{{ cast | json_decode | first | get(key='id') }}",
    properties: {
        playsCharacter: {
            type: "Relationship",
            target: {
                entity: "Character",
            },
            source: "{{ cast | json_decode | first | get(key='character') }}",
        },
        billingOrder: {
            source: "{{ cast | json_decode | first | get(key='order') }}",
            transformation: "integer",
        },
    },
},
```

Cassiopeia resolves the character name against the `Character` target and creates a `Character` URN. For example, `Jake Sully` becomes `urn:ngsi-ld:Character:JakeSully`. If a row has no character value, the nested relationship contributes no link, as a top-level relationship without a source would.

## Add a sub-attribute to a ListRelationship

`hasCast` reads every cast member's id and turns the results into one `objectList`. Its `castSize` Property is nested under the list relationship, so it describes the list as a whole:

```json5
hasCast: {
    type: "ListRelationship",
    target: {
        entity: "Person",
    },
    source: "{% for member in cast | json_decode %}{{ member.id }} {% endfor %}",
    properties: {
        castSize: {
            source: "{{ cast | json_decode | length }}",
            transformation: "integer",
        },
    },
},
```

The loop prints one id per cast member, separated by spaces. `ListRelationship` splits those ids into target objects, while `castSize` counts the members once for the complete list.

## Run it

```bash
cassiopeia map --manifest manifest.json5
```

The run reads 4803 films and writes them to `Movie.json`.

## Read the result

The normalized `Movie` entity for Avatar contains the nested relationship under `hasLeadActor`. It also contains the full cast under `hasCast`. The sample below shows two entries from the cast list so the structure stays readable:

```json
{
    "id": "urn:ngsi-ld:Movie:19995",
    "type": "Movie",
    "title": {
        "type": "Property",
        "value": "Avatar"
    },
    "hasLeadActor": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Person:65731",
        "objectType": "Person",
        "playsCharacter": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:Character:JakeSully",
            "objectType": "Character"
        },
        "billingOrder": {
            "type": "Property",
            "value": 0
        }
    },
    "hasCast": {
        "type": "ListRelationship",
        "objectList": [
            "urn:ngsi-ld:Person:65731",
            "urn:ngsi-ld:Person:8691"
        ],
        "objectType": "Person",
        "castSize": {
            "type": "Property",
            "value": 83
        }
    }
}
```

`playsCharacter` is a Relationship serialized inside `hasLeadActor`, with its own `object` and `objectType`. `castSize` is a Property serialized inside `hasCast`. A nested attribute can have its own sub-attributes, so this pattern can continue to another level when the data model needs it.

## Validation

The manifest selects `fail-when-schema` and points to the hand-authored `movie.schema.json`. Validation uses the normalized representation, so the schema can inspect `type`, `object`, `objectList`, and nested attributes directly.

The schema requires every entity to have an `id` and a `type`. When `hasLeadActor` is present, it must be a Relationship with a Person target. Its `playsCharacter` and `billingOrder` sub-attributes are checked when they are present. `hasCast` is checked in the same way, including its Person `objectList` and integer `castSize`. All 4803 films pass this validation.
