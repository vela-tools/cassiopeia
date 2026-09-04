# Creating synthetic JSON entities from a field

Most examples create one entity per source record. This one also creates `Country` entities from the comma-separated country field in each mountain record, without a separate country file. Mountains link to those synthetic countries, can link to a parent mountain, and get a GeoJSON point from a sexagesimal coordinate string.

## Get the data

One file, a list of the 120 highest mountains, from [free-json-datasets](https://github.com/sharmadhiraj/free-json-datasets):

```bash
wget -O mountains.json https://raw.githubusercontent.com/sharmadhiraj/free-json-datasets/master/docs/geography-environment/list_of_highest_mountains_on_earth.json
```

The file is an envelope with descriptive fields around a `data` array. Cassiopeia uses the array as its records and ignores the surrounding fields. One record looks like this:

```json
{
    "rank": "1",
    "mountain_names": "Mount Everest, Sagarmatha, Chomolungma",
    "height_rounded": "8,849 metres (29,032 ft)",
    "prominence_rounded": "8,849 metres (29,032 ft)",
    "range": "Mahalangur Himalaya",
    "coordinates": "27°59′17″N 86°55′30″E",
    "parent_mountain": "—",
    "first_ascent": "1953",
    "country_or_region": "Nepal, China"
}
```

Nothing here is directly usable as an entity ID, coordinate pair, or foreign key. The mapping has to shape each value first.

## A coordinate function

The `coordinates` field is a labelled degrees-minutes-seconds pair, `27°59′17″N 86°55′30″E`, rather than decimal longitude and latitude. The `dms_point` mapping function parses both axes, applies the hemisphere signs, and returns an RFC 7946 GeoJSON `Point` in longitude-latitude order (clause 3.1.1). The `point` transformation then uses it for the `GeoProperty`:

```json5
location: {
    source: "{{ dms_point(value=coordinates) }}",
    type: "GeoProperty",
    transformation: "point",
},
```

The function accepts typographic primes (`′`, `″`) as well as ASCII apostrophes and quotes. Minutes and seconds may be omitted, and a trailing footnote marker is ignored, so `27°42′12″N 88°08′51″E *` parses cleanly. `dms_point` is available to any mapping, alongside `geohash`.

## A relationship from a type to itself

`parent_mountain` names the massif a mountain rises from. The parent is another mountain in the same dataset, identified by name, so `Mountain` can relate to another `Mountain`:

```json5
parentMountain: {
    source: "{% if parent_mountain is matching(pat='[A-Za-z]') %}{{ parent_mountain }}{% endif %}",
    type: "Relationship",
    target: {
        entity: "Mountain",
    },
},
```

Both sides use the mountain's primary name. The identity takes the first comma-separated item from `mountain_names`; `parent_mountain` already contains that name, so its guarded relationship produces the same cleaned URN. Lhotse therefore points to `urn:ngsi-ld:Mountain:MountEverest`, the ID minted for Everest. The highest summits use an em dash for no parent, and the guard omits their relationship instead of creating a placeholder.

## Synthetic entities

The two entity types meet through `country_or_region`, a comma-separated list such as `Nepal, China`. One mapping declaration uses it in two ways:

```json5
hasCountry: {
    source: "{{ country_or_region }}",
    type: "ListRelationship",
    target: {
        entity: "Country",
    },
    syntheticEntity: {
        dataModel: "Country",
        identity: {
            entityName: "{{ this[0] }}",
        },
        attributes: {
            name: {
                source: "{{ this[0] }}",
                type: "Property",
                transformation: "string",
            },
        },
    },
},
```

`ListRelationship` splits the field into tokens and creates one country link per token. This works here because every country name is one word. A string list also splits on whitespace, so `United States` would incorrectly become two countries. Each token is passed to the synthetic mapping as its record and read with `this[0]`. The synthetic entity uses the same URN as the relationship and gives the country a body, even though no country dataset exists.

Because the country URN comes from its name, every mountain that names `Nepal` points to `urn:ngsi-ld:Country:Nepal`. The many synthetic fragments for Nepal merge into one entity. The 120 mountains name eight countries, so eight `Country` entities are written.

## The manifest

```json5
{
    version: "v1",
    inputs: [
        {
            source: "mountains.json",
            mapping: "mountain.json5",
            format: "json",
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

`Mountain` and `Country` are custom models with no published schemas. In `fail-when-schema` mode they are written without schema checks, just like the schemaless country in the [previous graph](../11-csv-relationship-graph/example.md).

## Run it

```bash
cassiopeia map --manifest manifest.json5
```

The run writes two files from the one input: `Mountain.json` with 120 entities and `Country.json` with 8.

## Read the result

Everest is the first row. Its coordinate is now a point, its height and prominence are whole-metre values extracted from `8,849 metres (29,032 ft)`, and it lists both countries it straddles. It has no parent, so the em dash produces no relationship:

```json
{
    "id": "urn:ngsi-ld:Mountain:MountEverest",
    "type": "Mountain",
    "name": {
        "type": "Property",
        "value": "Mount Everest"
    },
    "mountainRange": {
        "type": "Property",
        "value": "Mahalangur Himalaya"
    },
    "heightMetres": {
        "type": "Property",
        "value": 8849
    },
    "prominenceMetres": {
        "type": "Property",
        "value": 8849
    },
    "firstAscentYear": {
        "type": "Property",
        "value": 1953
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                86.925,
                27.988055555555555
            ]
        }
    },
    "hasCountry": {
        "type": "ListRelationship",
        "objectList": [
            "urn:ngsi-ld:Country:Nepal",
            "urn:ngsi-ld:Country:China"
        ],
        "objectType": "Country"
    }
}
```

Lhotse has a parent, and it points straight back at Everest:

```json
{
    "id": "urn:ngsi-ld:Mountain:Lhotse",
    "type": "Mountain",
    "parentMountain": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Mountain:MountEverest",
        "objectType": "Mountain"
    },
    "hasCountry": {
        "type": "ListRelationship",
        "objectList": [
            "urn:ngsi-ld:Country:China",
            "urn:ngsi-ld:Country:Nepal"
        ],
        "objectType": "Country"
    }
}
```

And Nepal, named by no dataset, only by the mountains that stand in it, is a `Country` entity in its own right:

```json
{
    "id": "urn:ngsi-ld:Country:Nepal",
    "type": "Country",
    "name": {
        "type": "Property",
        "value": "Nepal"
    }
}
```

An unclimbed peak carries `none` instead of a year. The guard keeps only four-digit years, so those mountains have no `firstAscentYear`. A single flat file therefore produces a connected graph of mountains, parents, and the countries named by the mountains.
