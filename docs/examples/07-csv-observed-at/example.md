# Marking CSV rows as observations with observedAt

In [example 2](../02-json-id-collision/example.md), records with the same ID were merged. A time series needs different handling. Every monthly gold price belongs to the same entity, but each row is a separate measurement in time.

Mark each record as an observation by giving an attribute an `observedAt`. Temporality then lives on the attribute (ETSI GS CIM 009 v1.9.1 clause 4.5.5). Cassiopeia can write either the **current state** (one entity per ID, with each attribute keeping its latest observation) or the full **series** (one temporal entity per ID whose attributes are instance arrays). The default is current-state.

## Get the data

The dataset is the [gold prices](https://github.com/datasets/gold-prices) monthly series, released into the public domain (ODC-PDDL). It has one row per month from 1833 onward.

Download it into this directory:

```bash
wget -O gold.csv https://raw.githubusercontent.com/datasets/gold-prices/main/data/monthly-processed.csv
```

## The record shape

The file has two columns: the month and that month's price.

```text
Date,Price
1833-01-01,18.930
1833-02-01,18.930
...
2026-07-01,4073.000
```

## One identity for the whole series

Every row describes the same entity, so the identity is a constant rather than a source field:

```json5
identity: {
    entityName: "gold",
},
```

Every record therefore resolves to the single id `urn:ngsi-ld:GoldPriceObserved:gold`.

## Mark each record as an observation

A constant identity would normally collapse every row into one value. It is valid here because the rows describe the same entity at different times. Adding `observedAt` to `price` reads the timestamp from `Date` and parses it as a datetime:

```json5
price: {
    source: "{{ Price }}",
    type: "Property",
    transformation: "float",
    properties: {
        observedAt: {
            source: "{{ Date }}",
            type: "Property",
            transformation: "datetime",
        },
    },
},
```

With an `observedAt` present, each row is an observation of `urn:ngsi-ld:GoldPriceObserved:gold` at its own time. The `datetime` transformation normalizes the source date to RFC 3339, turning `1833-01-01` into `1833-01-01T00:00:00Z`.

## Write the mapping

The full mapping is in [gold.json5](gold.json5). `GoldPriceObserved` is a plain type name, so this run does not validate against a schema.

```json5
{
    version: "v4",
    dataModel: "GoldPriceObserved",
    identity: {
        entityName: "gold",
    },
    attributes: {
        price: {
            source: "{{ Price }}",
            type: "Property",
            transformation: "float",
            properties: {
                observedAt: {
                    source: "{{ Date }}",
                    type: "Property",
                    transformation: "datetime",
                },
            },
        },
    },
}
```

## Run it: current state (the default)

Run the mapping from this directory:

```bash
cassiopeia map \
    --input gold.csv \
    --mapping gold.json5 \
    --type csv \
    --output out \
    --context none
```

The run creates `out/GoldPriceObserved.json` with a **single** object: the entity in its current state, `price` keeping the most recent observation and its `observedAt`.

```json
[
    {
        "id": "urn:ngsi-ld:GoldPriceObserved:gold",
        "type": "GoldPriceObserved",
        "price": {
            "type": "Property",
            "value": 4073.0,
            "observedAt": "2026-07-01T00:00:00Z"
        }
    }
]
```

The 2,323 rows are ranked by `observedAt`; the latest one wins. This is what you want for "the current gold price", and it is the shape a Smart Data Model schema describes, so it is also what validation checks.

## Run it: the full series

To keep every observation, ask for the series representation:

```bash
cassiopeia map \
    --input gold.csv \
    --mapping gold.json5 \
    --type csv \
    --output out-series \
    --context none \
    --temporal-representation series
```

Now `out-series/GoldPriceObserved.json` holds a single temporal entity whose `price` is a time-ordered array, one instance per month (ETSI GS CIM 009 v1.9.1 clause 5.2.20):

```json
[
    {
        "id": "urn:ngsi-ld:GoldPriceObserved:gold",
        "type": "GoldPriceObserved",
        "price": [
            {
                "type": "Property",
                "value": 18.93,
                "observedAt": "1833-01-01T00:00:00Z"
            },
            {
                "type": "Property",
                "value": 4073.0,
                "observedAt": "2026-07-01T00:00:00Z"
            }
        ]
    }
]
```

(The array is elided here; the real file carries all 2,323 instances, oldest first.)

The same `observedAt` mapping produces both shapes. The default keeps the latest observation per attribute; `--temporal-representation series` keeps them all, folded into one entity. In a manifest, set `output.temporal.representation` to `"series"` for the same result.
