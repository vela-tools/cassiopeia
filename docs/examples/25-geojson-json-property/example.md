# Keeping variable GeoJSON objects in a JsonProperty

Many source fields have a stable shape and can be mapped field by field. A weather alert's `parameters` object is different: its keys depend on the event type. `JsonProperty` keeps that object as JSON in `json` instead of turning each changing key into an attribute. This example maps the US National Weather Service alerts feed to a custom `WeatherAlert` model.

Like examples 24 and 26, this example uses a **custom** data model. See the [data-model guide](../../data-models.md) for background.

## What it teaches

- The `JsonProperty` type and its `json` member.
- When a variable or opaque object should stay whole instead of becoming a set of attributes.
- How a bare reference to a nested object resolves to that object, so `transformation: "object"` can carry it through unchanged.
- How to pass a GeoJSON feature's polygon through as a `GeoProperty`, as in the [parking example](../03-geojson-smart-data-model/example.md).

## Get the data

The [National Weather Service API](https://www.weather.gov/documentation/services-web-api) publishes active watches, warnings, and advisories as a GeoJSON `FeatureCollection` without requiring an API key. It does require a non-empty `User-Agent` header. Download the current nationwide feed into this directory:

```bash
curl -s "https://api.weather.gov/alerts/active" \
    -H "User-Agent: cassiopeia-docs-example" \
    -o alerts.json
```

The feed is live, so its contents and size change constantly. Use a filter such as `?area=WA` for one state or `?event=Flood%20Warning` for one event type when you want a smaller sample.

## The record shape

Each feature is one alert. Cassiopeia's GeoJSON ingestor exposes it as a record with `id`, `geometry`, and `properties`, accessed as `{{ id }}`, `{{ geometry }}`, and `{{ properties.<field> }}`. This example focuses on `properties.parameters`:

```json
{
    "type": "Feature",
    "geometry": {
        "type": "Polygon",
        "coordinates": [
            [
                [
                    -100.17,
                    36.56
                ],
                [
                    -100.26,
                    36.86
                ],
                [
                    -100.0,
                    36.94
                ],
                [
                    -100.0,
                    36.61
                ],
                [
                    -100.17,
                    36.56
                ]
            ]
        ]
    },
    "properties": {
        "id": "urn:oid:2.49.0.1.840.0.85e4c31af918b2713b69d590cdf8fac54100ce41.001.1",
        "event": "Special Weather Statement",
        "severity": "Moderate",
        "certainty": "Observed",
        "urgency": "Expected",
        "headline": "Special Weather Statement issued August 26 at 2:13AM CDT by NWS Amarillo TX",
        "areaDesc": "Beaver",
        "effective": "2026-08-26T02:13:00-05:00",
        "expires": "2026-08-26T02:45:00-05:00",
        "parameters": {
            "AWIPSidentifier": [
                "SPSAMA"
            ],
            "WMOidentifier": [
                "WWUS84 KAMA 260713"
            ],
            "NWSheadline": [
                "A STRONG THUNDERSTORM WILL IMPACT SOUTHEASTERN BEAVER COUNTY THROUGH 245 AM CDT"
            ],
            "eventMotionDescription": [
                "2026-08-26T07:13:00-00:00...storm...333DEG...16KT...36.82,-100.04"
            ],
            "maxWindGust": [
                "55 MPH"
            ],
            "maxHailSize": [
                "0.25"
            ],
            "BLOCKCHANNEL": [
                "EAS",
                "NWEM",
                "CMAS"
            ],
            "EAS-ORG": [
                "WXR"
            ]
        }
    }
}
```

`parameters` is a nested object with no fixed set of keys. Different event types produce different contents, so flattening it into named attributes would make the mapping brittle.

## The JsonProperty

A `JsonProperty` carries an arbitrary JSON value unchanged (ETSI GS CIM 009 v1.9.1 clause 4.5.24). It serializes with `json` where a `Property` would have `value`. Use it for a JSON document, variable object, heterogeneous array, or other fragment that the entity does not need to interpret. Splitting such a value into attributes would either break when its shape changes or invent structure that is not in the source.

The mapping sets `type: "JsonProperty"` and reads the object with `transformation: "object"`:

```json5
rawParameters: {
    source: "{{ properties.parameters }}",
    type: "JsonProperty",
    transformation: "object",
},
```

`{{ properties.parameters }}` follows a path into the record and resolves to the actual object, not a stringified copy. `transformation: "object"` preserves the object, and `JsonProperty` stores it unchanged under `json`. Its inner keys are neither inspected nor renamed.

## Write the mapping

The complete mapping is in [alert.json5](alert.json5). Scalar alert fields become ordinary Properties, the two timestamps use `datetime`, the feature polygon passes through as a `GeoProperty`, and `parameters` becomes the `JsonProperty`:

```json5
{
    version: "v4",
    dataModel: "WeatherAlert",
    identity: {
        entityName: "{{ properties.id }}",
    },
    attributes: {
        event: {
            source: "{{ properties.event }}",
            type: "Property",
            transformation: "string",
        },
        severity: {
            source: "{{ properties.severity }}",
            type: "Property",
            transformation: "string",
        },
        certainty: {
            source: "{{ properties.certainty }}",
            type: "Property",
            transformation: "string",
        },
        urgency: {
            source: "{{ properties.urgency }}",
            type: "Property",
            transformation: "string",
        },
        headline: {
            source: "{{ properties.headline }}",
            type: "Property",
            transformation: "string",
        },
        areaDescription: {
            source: "{{ properties.areaDesc }}",
            type: "Property",
            transformation: "string",
        },
        effective: {
            source: "{{ properties.effective }}",
            type: "Property",
            transformation: "datetime",
        },
        expires: {
            source: "{{ properties.expires }}",
            type: "Property",
            transformation: "datetime",
        },
        location: {
            source: "{{ geometry }}",
            type: "GeoProperty",
            transformation: "geometry",
        },
        rawParameters: {
            source: "{{ properties.parameters }}",
            type: "JsonProperty",
            transformation: "object",
        },
    },
}
```

The identity is the alert's CAP identifier, `properties.id`, a `urn:oid:` string that the URN cleaner turns into a safe tail. `location` reads the feature `geometry` with `transformation: "geometry"`. Alerts that reference forecast zones instead of a polygon have null geometry, so only `location` is omitted for those records.

## Run it

```bash
cassiopeia map \
    --input alerts.json \
    --mapping alert.json5 \
    --type geojson \
    --output out \
    --context none
```

The custom model needs no context or schema, so `--context none`. The run writes `out/WeatherAlert.json`, one entity per alert.

## Read the result

A thunderstorm-driven Special Weather Statement with its `parameters` intact under `json`:

```json
{
    "id": "urn:ngsi-ld:WeatherAlert:urnoid24901840085e4c31af918b2713b69d590cdf8fac54100ce410011",
    "type": "WeatherAlert",
    "event": {
        "type": "Property",
        "value": "Special Weather Statement"
    },
    "severity": {
        "type": "Property",
        "value": "Moderate"
    },
    "certainty": {
        "type": "Property",
        "value": "Observed"
    },
    "urgency": {
        "type": "Property",
        "value": "Expected"
    },
    "headline": {
        "type": "Property",
        "value": "Special Weather Statement issued August 26 at 2:13AM CDT by NWS Amarillo TX"
    },
    "areaDescription": {
        "type": "Property",
        "value": "Beaver"
    },
    "effective": {
        "type": "Property",
        "value": "2026-08-26T07:13:00.000Z"
    },
    "expires": {
        "type": "Property",
        "value": "2026-08-26T07:45:00.000Z"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Polygon",
            "coordinates": [
                [
                    [
                        -100.17,
                        36.56
                    ],
                    [
                        -100.0,
                        36.61
                    ],
                    [
                        -100.0,
                        36.94
                    ],
                    [
                        -100.26,
                        36.86
                    ],
                    [
                        -100.17,
                        36.56
                    ]
                ]
            ]
        }
    },
    "rawParameters": {
        "type": "JsonProperty",
        "json": {
            "AWIPSidentifier": [
                "SPSAMA"
            ],
            "WMOidentifier": [
                "WWUS84 KAMA 260713"
            ],
            "NWSheadline": [
                "A STRONG THUNDERSTORM WILL IMPACT SOUTHEASTERN BEAVER COUNTY THROUGH 245 AM CDT"
            ],
            "eventMotionDescription": [
                "2026-08-26T07:13:00-00:00...storm...333DEG...16KT...36.82,-100.04"
            ],
            "maxWindGust": [
                "55 MPH"
            ],
            "maxHailSize": [
                "0.25"
            ],
            "BLOCKCHANNEL": [
                "EAS",
                "NWEM",
                "CMAS"
            ],
            "EAS-ORG": [
                "WXR"
            ]
        }
    }
}
```

`rawParameters` carries every key in the source alert, including `maxWindGust`, `maxHailSize`, and the storm-motion vector. None needs its own mapping. A flood alert from the same run could contain completely different keys without requiring a mapping change. The timestamps were parsed to UTC and the polygon passed through as the location.

The polygon's positions are the source's, in reverse order. The alert feed winds its exterior ring clockwise; RFC 7946 clause 3.1.6 states the opposite as a producer requirement, so Cassiopeia rewinds every exterior ring it emits counterclockwise. See the [mapping guide](../../mapping.md#geoproperty) for the geometry normalisation rules.

## When a JsonProperty, and when not

Use a `JsonProperty` when a value varies between records or is an opaque document that the consumer will parse. Prefer real attributes, a nested `Property` with [`mappings`](../03-geojson-smart-data-model/example.md), or top-level attributes when the structure is stable and should be queryable or validated. The inner keys of a `JsonProperty` are invisible to NGSI-LD queries and schemas. The [attribute-type reference](../../ngsi-ld/attribute-types.md#jsonproperty) explains the distinction.
