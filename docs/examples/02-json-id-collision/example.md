# Keeping JSON entities with colliding IDs apart

This example turns a JSON array of cities into NGSI-LD `City` entities. The first identity combines country and city name, then adds a short coordinate-derived value when repeated names collide.

## Get the data

The dataset is [world-cities](https://github.com/joelacus/world-cities), a list of cities with a population of at least 15,000. It has 34,080 records. The dataset is licensed under Creative Commons Attribution 4.0.

Download it into this directory:

```bash
wget -O city.json https://raw.githubusercontent.com/joelacus/world-cities/main/world_cities_15000.json
```

## The record shape

The generic JSON ingestor expects a top-level array of objects and treats each object as one record. A record from this file looks like this:

```json
{
    "country": "AD",
    "name": "les Escaldes",
    "lat": "42.50729",
    "lng": "1.53414"
}
```

The fields are the country code, city name, latitude, and longitude. The country code uses ISO 3166-1 alpha-2 codes. The coordinate values look numeric, but the source stores them as strings.

## A first identity, and why it collides

Every record needs an ID that remains stable for the same real-world city. A bare city name is not enough because names repeat across countries, so the first attempt prefixes the cleaned name with the country code:

```json5
identity: {
    entityName: "{{ country }}-{{ name | clean }}",
}
```

For `les Escaldes`, this produces `AD-les Escaldes`. Cassiopeia cleans the name into a URN-safe segment, dropping the space and transliterating accents, then combines it with the `City` model. `São Paulo` in Brazil, for example, becomes `urn:ngsi-ld:City:BR-SaoPaulo`.

This identity still collides. Countries often contain several cities with the same name. The United States alone has eight Springfields, six Middletowns, and hundreds of other repeated names. All eight Springfields therefore produce the same ID, `urn:ngsi-ld:City:US-Springfield`.

When two records resolve to one ID, Cassiopeia merges them instead of writing two entities. If their values disagree, the first record wins. Running the mapping produces 32,972 entities from 34,080 records: 1,108 records merge away during resolution because their IDs collide. Seven of the eight Springfields disappear, including their coordinates, and the output does not flag the collision.

## A stable suffix from the coordinates

The records already contain a value that distinguishes same-named cities: their location. Two cities can share a name and country while having different coordinates, so the coordinates can supply the missing part of the ID.

Full coordinates make unwieldy IDs. A [geohash](https://en.wikipedia.org/wiki/Geohash) compresses latitude and longitude into a short string using `0-9` and `b-z`; more characters mean a smaller cell on Earth's surface. The result is deterministic and already URN-safe. Cassiopeia exposes it as the `geohash` template function:

```json5
identity: {
    entityName: "{{ country }}-{{ name | clean }}-{{ geohash(lat=lat, lon=lng) }}",
}
```

The function receives `lat` and `lon` values from this source's numeric strings, plus an optional `precision`. The default precision of nine characters covers a cell only a few metres across, so each city gets a distinct suffix here. `les Escaldes` becomes `urn:ngsi-ld:City:AD-lesEscaldes-sp91ffjnu`, and all 34,080 records remain separate. `precision=7` is enough for this dataset; `precision=5` leaves three pairs sharing an ID.

Because the geohash comes from the coordinates rather than a running counter, the ID stays the same across runs. Re-mapping the same city therefore produces the same entity identity.

## Write the mapping

The full mapping is in [city.json5](city.json5):

```json5
{
    version: "v4",
    dataModel: "City",
    identity: {
        // Country and name alone collide for same-named cities within a country; the
        // coordinate-derived geohash suffix keeps each city's id distinct.
        entityName: "{{ country }}-{{ name | clean }}-{{ geohash(lat=lat, lon=lng) }}",
    },
    attributes: {
        name: {
            source: "{{ name }}",
        },
        country: {
            source: "{{ country }}",
        },
        location: {
            type: "GeoProperty",
            // GeoJSON coordinates are ordered longitude, then latitude.
            source: [
                "{{ lng }}",
                "{{ lat }}",
            ],
            transformation: "point",
        },
    },
}
```

`name` and `country` have a source but no explicit type or transformation, so they become text `Property` attributes.

`location` is a `GeoProperty`. GeoJSON orders point coordinates as `[longitude, latitude]`, so the source array supplies `lng` first and `lat` second. The `point` transformation creates the GeoJSON `Point` and parses both strings as numbers.

## Run it

Run the mapping from this directory:

```bash
cassiopeia map \
    --input city.json \
    --mapping city.json5 \
    --type json \
    --output out \
    --context none
```

The command uses `--context none` so the example does not need a JSON-LD context or a Smart Data Models catalog. [Output](../../output.md#deliver-context) explains how to attach a real context later.

Cassiopeia writes one file per entity type. This run therefore creates `out/City.json`, a single JSON array of 34,080 `City` entities.

## Read the result

The default representation is normalized, so each attribute includes its NGSI-LD type. Entity resolution can change the input order. Two output entities look like this:

```json
[
    {
        "id": "urn:ngsi-ld:City:AD-lesEscaldes-sp91ffjnu",
        "type": "City",
        "name": {
            "type": "Property",
            "value": "les Escaldes"
        },
        "country": {
            "type": "Property",
            "value": "AD"
        },
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [
                    1.53414,
                    42.50729
                ]
            }
        }
    },
    {
        "id": "urn:ngsi-ld:City:DE-Hamburg-u1x0eksew",
        "type": "City",
        "name": {
            "type": "Property",
            "value": "Hamburg"
        },
        "country": {
            "type": "Property",
            "value": "DE"
        },
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [
                    9.99302,
                    53.55073
                ]
            }
        }
    }
]
```
