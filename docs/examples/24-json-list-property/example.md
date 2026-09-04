# Mapping JSON arrays with a ListProperty

Some lists are meaningful only when their order is preserved. In a day of hourly bicycle counts, each position represents one hour. NGSI-LD's `ListProperty` stores that sequence in `valueList`. This example maps a public bicycle counter to a custom `BicycleCounter` model and builds one ordered list per day.

It uses a **custom** data model because no published Smart Data Model represents an hourly counter in this shape. The [data-model guide](../../data-models.md) explains when a custom model is appropriate.

## What it teaches

- The `ListProperty` type and its `valueList` member.
- Why preserving order matters, and why a bare value array does not make that promise to consumers.
- How preprocessing can turn a one-row-per-hour feed into one record per day with an ordered array, as in examples [20](../21-json-dataset-id/example.md) and [21](../22-csv-broker-temporal/example.md).

## Get the data

The [Fremont Bridge Bicycle Counter](https://data.seattle.gov/Transportation/Fremont-Bridge-Bicycle-Counter/65db-xm6k) is one of Seattle's oldest bike counters. The City of Seattle publishes it through its Socrata open-data portal without requiring a key. Each row represents one hour and contains a timestamp, total count, and northbound and southbound counts.

The counter reports one row per hour, but the target entity has one record per day with an ordered 24-element array. `jq` reshapes the data while fetching it, groups rows by day, and places each hourly total at its index. Nothing is stored in the repository.

```bash
curl -s -G "https://data.seattle.gov/resource/65db-xm6k.json" \
    --data-urlencode "\$where=date between '2026-07-01T00:00:00' and '2026-07-07T23:59:59'" \
    --data-urlencode "\$order=date" \
    --data-urlencode "\$limit=200" \
    | jq '
      map({day: (.date[0:10]), hour: (.date[11:13] | tonumber), count: (.fremont_bridge | tonumber)})
      | group_by(.day)
      | map({
          counter: "Fremont Bridge",
          date: .[0].day,
          latitude: 47.647417,
          longitude: -122.349722,
          hourlyCounts: reduce .[] as $r ([range(24) | 0]; .[$r.hour] = $r.count)
        })' \
    > counts.json
```

`[range(24) | 0]` creates a 24-element array of zeros. The `reduce` expression places each total at `hourlyCounts[hour]`, preserving the hour positions even when a row is missing. The command covers one week; change the dates for another range.

## The record shape

Each element of the reshaped array represents one counter-day. Cassiopeia's JSON ingestor turns the top-level array into one record per element, and each record becomes a `BicycleCounter`:

```json
{
    "counter": "Fremont Bridge",
    "date": "2026-07-01",
    "latitude": 47.647417,
    "longitude": -122.349722,
    "hourlyCounts": [
        35,
        17,
        12,
        9,
        21,
        39,
        142,
        312,
        456,
        343,
        232,
        185,
        166,
        189,
        221,
        347,
        648,
        589,
        301,
        325,
        217,
        134,
        92,
        58
    ]
}
```

`hourlyCounts` is already a JSON array of integers. That matters for the mapping: a bare `{{ hourlyCounts }}` reference resolves to the raw array, not to a stringified copy of it.

## The ListProperty

A `ListProperty` is an attribute whose value is an ordered list (ETSI GS CIM 009 v1.9.1 clause 4.5.21). It serializes with a `valueList` array where a `Property` would have its single `value`. Use it when values form one sequence, such as hourly readings, ranked items, or ordered route stops.

The mapping sets `type: "ListProperty"` and reads the array with `transformation: "array"`:

```json5
hourlyCounts: {
    source: "{{ hourlyCounts }}",
    type: "ListProperty",
    transformation: "array",
},
```

`transformation: "array"` reads the source as a JSON array. The bare `{{ hourlyCounts }}` reference resolves to the actual array, so all 24 counts pass through in the order created by preprocessing. That order is part of the type's meaning: a `ListProperty` tells consumers to read the values positionally.

## Write the mapping

The complete mapping is in [counter.json5](counter.json5):

```json5
{
    version: "v4",
    dataModel: "BicycleCounter",
    identity: {
        entityName: "{{ counter }}-{{ date }}",
    },
    attributes: {
        counterName: {
            source: "{{ counter }}",
            type: "Property",
            transformation: "string",
        },
        dateObserved: {
            source: "{{ date }}",
            type: "Property",
            transformation: "date",
        },
        location: {
            source: [
                "{{ longitude }}",
                "{{ latitude }}",
            ],
            type: "GeoProperty",
            transformation: "point",
        },
        hourlyCounts: {
            source: "{{ hourlyCounts }}",
            type: "ListProperty",
            transformation: "array",
        },
    },
}
```

The identity combines the counter name and day, so each day becomes its own entity. `Fremont Bridge-2026-07-01` becomes `urn:ngsi-ld:BicycleCounter:FremontBridge-2026-07-01` after the URN cleaner removes the space. `dateObserved` uses `transformation: "date"`, `location` builds a point from the fixed coordinates, and `hourlyCounts` is the `ListProperty`.

## Run it

A single source file, so no manifest:

```bash
cassiopeia map \
    --input counts.json \
    --mapping counter.json5 \
    --type json \
    --output out \
    --context none
```

The model is a custom one, so `--context none` attaches no `@context` and there is no schema to download or validate against. The run writes `out/BicycleCounter.json`, one entity per day.

## Read the result

One day comes out with its twenty-four counts in order under `valueList`:

```json
{
    "id": "urn:ngsi-ld:BicycleCounter:FremontBridge-2026-07-01",
    "type": "BicycleCounter",
    "counterName": {
        "type": "Property",
        "value": "Fremont Bridge"
    },
    "dateObserved": {
        "type": "Property",
        "value": "2026-07-01"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                -122.349722,
                47.647417
            ]
        }
    },
    "hourlyCounts": {
        "type": "ListProperty",
        "valueList": [
            35,
            17,
            12,
            9,
            21,
            39,
            142,
            312,
            456,
            343,
            232,
            185,
            166,
            189,
            221,
            347,
            648,
            589,
            301,
            325,
            217,
            134,
            92,
            58
        ]
    }
}
```

The list is positional: `valueList[7]` is 312 at 07:00, `valueList[8]` is 456 at 08:00, and `valueList[16]` is 648 at 17:00. Those readings are meaningful because the order is preserved. A weekend day from the same run has a flatter pattern, with more traffic in the middle of the day, and the shared positions make the days comparable.

## When a ListProperty, and when not

Use a `ListProperty` when values form an ordered sequence read by position. Use a plain `Property` with an array when order has no meaning, and a [`ListRelationship`](../12-csv-list-relationship/example.md) when the elements link to other entities. The [attribute-type reference](../../ngsi-ld/attribute-types.md#listproperty) compares the three.
