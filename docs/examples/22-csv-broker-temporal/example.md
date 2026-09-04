# Sending CSV temporal output to a broker

[Example 7](../07-csv-observed-at/example.md) marked each CSV row as an observation with `observedAt`. Temporality belongs to the attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.5), so a plain run writes the **current state**: one entity per ID, with each attribute keeping its latest observation. NGSI-LD also defines **EntityTemporal** (clause 5.2.20), where one entity contains arrays of time-stamped attribute instances. This example builds that series from a storm track and sends it to a live Context Broker.

This example makes two separate choices:

- **Where** the entities go. Every example so far wrote files; this one pushes to a broker over HTTP.
- **What** shape is produced. Current-state (the default) emits one entity per ID with its latest observation; `series` folds a storm's observations into one EntityTemporal with instance arrays.

The choices interact at the broker. A folded EntityTemporal belongs only at the `/temporal/entities` endpoint, so a `series` representation against a broker requires `operation: "temporal"`. Sent to that endpoint, the broker rebuilds the same history whether Cassiopeia folds a storm first or streams its observations separately. The difference is easiest to see in file output: current-state writes _one_ object holding the latest reading, while `series` writes _one_ object holding arrays.

## Get the data

NOAA's [HURDAT2](https://www.nhc.noaa.gov/data/#hurdat) Atlantic best-track database records Atlantic tropical cyclones since 1851 and is in the public domain. Download the current revision; NOAA changes the date in the filename when it updates the file:

```bash
wget -O hurdat2-atlantic.txt https://www.nhc.noaa.gov/data/hurdat/hurdat2-1851-2023-051124.txt
```

HURDAT2 is not a plain CSV. Before each storm it has a one-line **header** containing the storm's id, name, and number of track rows. The following rows carry no storm id of their own:

```text
AL122005,            KATRINA,     34,
20050823, 1800,  , TD, 23.1N,  75.1W,  30, 1008, ...
20050824, 1200,  , TS, 24.5N,  76.5W,  45, -999, ...
20050828, 1800,  , HU, 26.3N,  89.6W, 145,  902, ...
```

The file has no single header for all rows, so the generic CSV ingestor cannot read it directly. The example includes a small **preprocessing script**, [flatten.awk](flatten.awk), which copies each storm's ID and name onto its track rows and separates the packed date and time. Run it once:

```bash
awk -f flatten.awk hurdat2-atlantic.txt > cyclones.csv
```

The `awk` step also writes `2005-08-23` and `18:00` instead of the packed values `20050823` and `1800`. The separators keep CSV type inference from turning the fields into integers, so the mapping can join them into a timestamp.

## The record shape

After flattening, the ingestor sees an ordinary headed CSV with one row per six-hour observation:

```text
storm_id,name,date,time,record_id,status,lat,lon,max_wind,min_pressure
AL122005,KATRINA,2005-08-23,18:00,,TD,23.1N,75.1W,30,1008
AL122005,KATRINA,2005-08-24,12:00,,TS,24.5N,76.5W,45,-999
AL122005,KATRINA,2005-08-28,18:00,,HU,26.3N,89.6W,145,902
```

`lat` and `lon` include hemisphere letters (`23.1N`, `75.1W`). `max_wind` is one-minute sustained wind in knots, and `min_pressure` is central pressure in hectopascals, or `-999` when it was not measured.

## One identity per storm

Every row describes the same cyclone at another time, so the identity is only the storm ID:

```json5
identity: {
    entityName: "{{ storm_id }}",
},
```

Every KATRINA row resolves to `urn:ngsi-ld:TropicalCyclone:AL122005`. As in example 7, a constant identity works here because `observedAt` marks each row as a separate observation.

## One timestamp, composed and shared

HURDAT2 splits the instant across two columns. The mapping joins them into one RFC 3339 timestamp and parses it as a datetime:

```json5
observedAt: {
    source: "{{ date }}T{{ time }}:00Z",
    type: "Property",
    transformation: "datetime",
},
```

With `date` set to `2005-08-23` and `time` to `18:00`, the mapping produces `2005-08-23T18:00:00Z`. The datetime parser needs the separators added by `awk`; without them the result would be the invalid `20050823T1800:00Z`. Every temporal attribute below uses this same `observedAt`.

## The attributes

`name` is the only attribute with **no** `observedAt`. A storm's name is static, so the series fold keeps it as one value rather than an array:

```json5
name: {
    source: "{{ name }}",
    type: "Property",
    transformation: "string",
},
```

`maxSustainedWind` carries a UN/CEFACT unit code, `KNT` for knot, alongside its timestamp:

```json5
maxSustainedWind: {
    source: "{{ max_wind }}",
    type: "Property",
    transformation: "float",
    properties: {
        unitCode: {
            source: "KNT",
        },
        observedAt: {
            source: "{{ date }}T{{ time }}:00Z",
            type: "Property",
            transformation: "datetime",
        },
    },
},
```

`minPressure` (unit code `A97`, hectopascal) has to reject HURDAT2's `-999` missing-value sentinel. A guard drops the whole attribute for those rows rather than emitting `-999` as a pressure:

```json5
minPressure: {
    source: "{% if min_pressure and min_pressure != -999 %}{{ min_pressure }}{% endif %}",
    type: "Property",
    transformation: "integer",
    properties: {
        unitCode: {
            source: "A97",
        },
        observedAt: {
            source: "{{ date }}T{{ time }}:00Z",
            type: "Property",
            transformation: "datetime",
        },
    },
},
```

`category` derives the Saffir-Simpson class from the wind, with an `if`/`elif` ladder in knots:

```json5
category: {
    source: "{% if max_wind >= 137 %}Category 5{% elif max_wind >= 113 %}Category 4{% elif max_wind >= 96 %}Category 3{% elif max_wind >= 83 %}Category 2{% elif max_wind >= 64 %}Category 1{% elif max_wind >= 34 %}Tropical Storm{% else %}Tropical Depression{% endif %}",
    type: "Property",
    transformation: "string",
    properties: {
        observedAt: {
            source: "{{ date }}T{{ time }}:00Z",
            type: "Property",
            transformation: "datetime",
        },
    },
},
```

`location` is a `GeoProperty`. GeoJSON wants signed decimal degrees, longitude first, but HURDAT2 stores each coordinate as a hemisphere-labelled string. The mapping strips the letter and prefixes a minus for the western and southern hemispheres:

```json5
location: {
    source: [
        "{% if 'W' in lon %}-{% endif %}{{ lon | replace(from='W', to='') | replace(from='E', to='') }}",
        "{% if 'S' in lat %}-{% endif %}{{ lat | replace(from='N', to='') | replace(from='S', to='') }}",
    ],
    type: "GeoProperty",
    transformation: "point",
    properties: {
        observedAt: {
            source: "{{ date }}T{{ time }}:00Z",
            type: "Property",
            transformation: "datetime",
        },
    },
},
```

`23.1N`/`75.1W` becomes `[-75.1, 23.1]`. The `dms_point` function from [example 13](../13-json-synthetic-entities/example.md) is not suitable because these values are signed decimals, not degrees-minutes-seconds strings. The complete mapping is in [cyclone.json5](cyclone.json5). `TropicalCyclone` is a custom type with a hand-authored context, so this run has no schema validation, as in [example 20](../20-csv-at-context/example.md).

## See both shapes as a file first

Before sending data to a broker, run the mapping twice to a file and compare the two shapes. First, use the default current-state representation:

```bash
cassiopeia map -i cyclones.csv -m cyclone.json5 --type csv --writer file --output out --context none
```

`out/TropicalCyclone.json` holds **one** object for the storm, each attribute keeping its latest observation. KATRINA's three rows collapse to the reading from 2005-08-28, its peak:

```json
{
    "id": "urn:ngsi-ld:TropicalCyclone:AL122005",
    "type": "TropicalCyclone",
    "name": {
        "type": "Property",
        "value": "KATRINA"
    },
    "maxSustainedWind": {
        "type": "Property",
        "value": 145.0,
        "observedAt": "2005-08-28T18:00:00Z",
        "unitCode": "KNT"
    },
    "minPressure": {
        "type": "Property",
        "value": 902,
        "observedAt": "2005-08-28T18:00:00Z",
        "unitCode": "A97"
    },
    "category": {
        "type": "Property",
        "value": "Category 5",
        "observedAt": "2005-08-28T18:00:00Z"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                -89.6,
                26.3
            ]
        },
        "observedAt": "2005-08-28T18:00:00Z"
    }
}
```

Each attribute's records are ranked by `observedAt`; the latest wins. Now ask for the series representation:

```bash
cassiopeia map -i cyclones.csv -m cyclone.json5 --type csv --writer file --output out --temporal-representation series --context none
```

Now `out/TropicalCyclone.json` holds **one** object per storm, folded into an EntityTemporal, with each temporal attribute stored as a time-ordered array:

```json
{
    "id": "urn:ngsi-ld:TropicalCyclone:AL122005",
    "type": "TropicalCyclone",
    "name": {
        "type": "Property",
        "value": "KATRINA"
    },
    "maxSustainedWind": [
        {
            "type": "Property",
            "value": 30.0,
            "observedAt": "2005-08-23T18:00:00Z",
            "unitCode": "KNT"
        },
        {
            "type": "Property",
            "value": 45.0,
            "observedAt": "2005-08-24T12:00:00Z",
            "unitCode": "KNT"
        },
        {
            "type": "Property",
            "value": 145.0,
            "observedAt": "2005-08-28T18:00:00Z",
            "unitCode": "KNT"
        }
    ],
    "minPressure": [
        {
            "type": "Property",
            "value": 1008,
            "observedAt": "2005-08-23T18:00:00Z",
            "unitCode": "A97"
        },
        {
            "type": "Property",
            "value": 902,
            "observedAt": "2005-08-28T18:00:00Z",
            "unitCode": "A97"
        }
    ],
    "category": [
        {
            "type": "Property",
            "value": "Tropical Depression",
            "observedAt": "2005-08-23T18:00:00Z"
        },
        {
            "type": "Property",
            "value": "Tropical Storm",
            "observedAt": "2005-08-24T12:00:00Z"
        },
        {
            "type": "Property",
            "value": "Category 5",
            "observedAt": "2005-08-28T18:00:00Z"
        }
    ],
    "location": [
        {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [
                    -75.1,
                    23.1
                ]
            },
            "observedAt": "2005-08-23T18:00:00Z"
        },
        {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [
                    -76.5,
                    24.5
                ]
            },
            "observedAt": "2005-08-24T12:00:00Z"
        },
        {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [
                    -89.6,
                    26.3
                ]
            },
            "observedAt": "2005-08-28T18:00:00Z"
        }
    ]
}
```

In the series result, `name` stays a single static value. Each temporal attribute is sorted oldest first, so its array follows the storm through time. `minPressure` has only two instances because the `-999` row contributed nothing. The missing pressure appears as a gap in the series.

## Send it to a broker

The [manifest](manifest.json5) sends the same run to a Context Broker instead of a directory:

```json5
output: {
    target: "context-broker",
    url: "http://localhost:9090/",
    operation: "temporal",
    temporal: {
        representation: "series",
    },
    context: "cyclone-context.jsonld",
    contextDelivery: "body",
},
```

`operation: "temporal"` selects the `/ngsi-ld/v1/temporal/entities` endpoint (clause 5.6.11). With the temporal representation set to `series`, Cassiopeia folds each storm before delivery, so one request carries its complete track. A folded EntityTemporal belongs only at that endpoint, so this pairing is required; a `series` representation with any other broker operation is rejected. `TropicalCyclone` is a custom model, so the hand-authored [cyclone-context.jsonld](cyclone-context.jsonld) travels in each entity body (`contextDelivery: "body"`, `application/ld+json`). A `Link` header would instead require a context URL the broker can fetch.

## Run it against Scorpio

The [docker-compose.yml](docker-compose.yml) starts [Scorpio](https://github.com/ScorpioBroker/ScorpioBroker), an open-source NGSI-LD broker backed by PostGIS. It serves the API on port 9090. Start it and wait for it to become healthy:

```bash
docker compose up -d
```

Then run the mapping through the manifest:

```bash
cassiopeia map --manifest manifest.json5
```

Cassiopeia sends one POST per storm to the temporal endpoint, with the folded track rather than one request per observation.

## Query the track back

Ask the broker for KATRINA's track over the storm's life:

```bash
curl -s -H 'Accept: application/ld+json' \
    'http://localhost:9090/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:TropicalCyclone:AL122005?timerel=between&timeAt=2005-08-23T00:00:00Z&endTimeAt=2005-08-29T00:00:00Z'
```

The broker returns the temporal representation: each attribute is an array of time-stamped instances ordered through the storm's life, matching the series file above. The `temporal` operation puts the observations on the temporal timeline. Whether Cassiopeia sends them separately or folds them first, the broker rebuilds the same history. To stop the stack and discard its data:

```bash
docker compose down -v
```

[Output](../../output.md#send-to-a-context-broker) documents broker destinations, operations, atomic delivery, context delivery, authentication, and the temporal representation used here.
