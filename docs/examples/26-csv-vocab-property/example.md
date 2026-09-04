# Mapping CSV vocabulary terms with a VocabProperty

A category from a shared vocabulary is more precise than free-form text. A `VocabProperty` stores the term's IRI in `vocab`, so different systems can use the same identifier. This example maps OpenStreetMap amenities in Slovenia to a custom `UrbanMobilityPoint` model and builds the IRI from each `amenity` value.

This is the third custom-model example. The [data-model guide](../../data-models.md) explains why a controlled vocabulary can belong in a custom model.

## What it teaches

- The `VocabProperty` type and its `vocab` IRI.
- How to distinguish a shared-vocabulary term from an opaque code (`Property`) or a link to an entity ([`Relationship`](../10-json-relationships/example.md)).
- How to read OpenStreetMap through the keyless Overpass API in CSV form, including tab delimiters and `@`-prefixed column names.

## Get the data

[Overpass](https://overpass-api.de/) is a read-only query API for OpenStreetMap data. It needs no key and can return CSV. The query resolves Slovenia's national boundary into an area, selects six urban-mobility amenity types _within that area_, and prints five columns. Using an area rather than a rectangular box keeps the query from spilling into Croatia, Austria, Italy, and Hungary. `out center;` gives ways and relations a centroid, so every result has coordinates.

```bash
curl -s "https://overpass-api.de/api/interpreter" \
    --data-urlencode 'data=[out:csv(::type,::id,::lat,::lon,amenity,name)][timeout:180];area["ISO3166-1"="SI"][admin_level=2]->.si;nwr[amenity~"^(charging_station|bicycle_rental|bicycle_parking|car_sharing|fuel|taxi)$"](area.si);out center;' \
    -o amenities.csv
```

The result is tab-separated and contains a few thousand rows, although the count changes as the map is edited. Replace the `ISO3166-1` value with another country code to search a different country.

## The record shape

Overpass CSV uses tabs as delimiters and prefixes special columns with `@`. Cassiopeia detects the delimiter, so the file needs no reshaping:

```text
@type	@id	@lat	@lon	amenity	name
node	14074793302	46.0472699	14.4975925	charging_station	Lj - Snežniška
node	1292047961	46.0457409	14.5061968	bicycle_rental	Bicikelj - Grudnovo nabrežje
```

`amenity` comes from OpenStreetMap's fixed [`amenity` key vocabulary](https://wiki.openstreetmap.org/wiki/Key:amenity), while `name` is often empty free text. The `@type`, `@id`, `@lat`, and `@lon` headers are not valid template identifiers, so the mapping uses the `this['...']` bracket form from the [messy-headers example](../06-csv-messy-headers/example.md). `@type` can be `node`, `way`, or `relation`, and an OSM ID is unique only within one type, so the identity combines both fields.

## The VocabProperty

A `VocabProperty` stores a vocabulary IRI (ETSI GS CIM 009 v1.9.1 clause 4.5.20). It serializes with a `vocab` member, which must be a well-formed IRI; an invalid bare string produces no attribute. This type suits categories, statuses, and other terms defined by a controlled vocabulary.

The difference from a plain `Property` is the identifier. A `Property` holding `"charging_station"` contains only a literal string, so differently spelled values do not automatically match. A `VocabProperty` holding `https://wiki.openstreetmap.org/wiki/Tag:amenity=charging_station` identifies the term globally. The mapping builds that IRI by inserting the source value into the term's canonical URL:

```json5
category: {
    source: "https://wiki.openstreetmap.org/wiki/Tag:amenity={{ amenity }}",
    type: "VocabProperty",
},
```

No `transformation` is needed because the resolved source is already an IRI. `VocabProperty` writes it directly to `vocab`: `fuel` yields `.../Tag:amenity=fuel`, `taxi` yields `.../Tag:amenity=taxi`, and so on.

## Write the mapping

The complete mapping is in [amenity.json5](amenity.json5):

```json5
{
    version: "v4",
    dataModel: "UrbanMobilityPoint",
    identity: {
        entityName: "{{ this['@type'] }}-{{ this['@id'] }}",
    },
    attributes: {
        name: {
            source: "{{ name }}",
            type: "Property",
            transformation: "string",
        },
        category: {
            source: "https://wiki.openstreetmap.org/wiki/Tag:amenity={{ amenity }}",
            type: "VocabProperty",
        },
        location: {
            source: [
                "{{ this['@lon'] }}",
                "{{ this['@lat'] }}",
            ],
            type: "GeoProperty",
            transformation: "point",
        },
    },
}
```

The identity combines the OSM element type and ID, so a node and a way with the same ID remain distinct. `name` is an ordinary Property and is omitted when empty. `location` builds a point from `@lon` and `@lat`, in GeoJSON's longitude-first order. `category` is the `VocabProperty`.

## Run it

```bash
cassiopeia map \
    --input amenities.csv \
    --mapping amenity.json5 \
    --type csv \
    --output out \
    --context none
```

The custom model needs no context or schema, so `--context none`. The run writes `out/UrbanMobilityPoint.json`, one entity per amenity.

## Read the result

A Ljubljana charging station with its category carried as an IRI:

```json
{
    "id": "urn:ngsi-ld:UrbanMobilityPoint:node-14074793302",
    "type": "UrbanMobilityPoint",
    "name": {
        "type": "Property",
        "value": "Lj - Snežniška"
    },
    "category": {
        "type": "VocabProperty",
        "vocab": "https://wiki.openstreetmap.org/wiki/Tag:amenity=charging_station"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                14.4975925,
                46.0472699
            ]
        }
    }
}
```

`category` is a `VocabProperty`, and `vocab` is the IRI of the OSM wiki page defining `amenity=charging_station`, not the loose string `charging_station`. Each entity carries one of six such IRIs, allowing consumers to group categories by identifier rather than by string matching.

## When a VocabProperty, and when not

Use a `VocabProperty` when a value is a controlled-vocabulary term that should be identified by IRI. Use a plain `Property` for an opaque code with no shared vocabulary, and a `Relationship` when the value points to a first-class entity rather than a vocabulary term. The [attribute-type reference](../../ngsi-ld/attribute-types.md#vocabproperty) compares the three.
