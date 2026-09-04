# Converting GeoJSON geometry types for a GeoProperty

Real geospatial sources rarely use the geometry type a data model asks for. An administrative-boundary file describes every region as a `MultiPolygon` because some regions are archipelagos, even when most are single landmasses. A model that wants one `Polygon` cannot simply take it: dropping the other members loses geometry, and Cassiopeia refuses to do that unless the mapping explicitly allows it.

This example maps Eurostat's NUTS level 1 regions to a custom `Region` model. It declares two GeoProperties from the same source geometry: a `location` demoted to a `Polygon`, and a separate `centroid` derived as a map-pin `Point`.

It uses a **custom** data model. See the [data-model guide](../../data-models.md) for background.

## What it teaches

- Which geometry conversions Cassiopeia applies on its own, and which one has to be declared.
- The `geometry` block that declares a conversion beside a `transformation`.
- Why a centroid belongs in its own GeoProperty rather than replacing the boundary.
- That every emitted polygon ring is closed and rewound to RFC 7946's right-hand rule.

## Get the data

Eurostat's GISCO service publishes the NUTS regions as GeoJSON in WGS84, with no key. Download the level 1 regions at 1:20 million into this directory:

```bash
curl -s "https://gisco-services.ec.europa.eu/distribution/v2/nuts/geojson/NUTS_RG_20M_2021_4326_LEVL_1.geojson" \
    -o regions.geojson
```

The file holds 125 features and is about 460 kB.

## The record shape

Every feature is one region, and every geometry is a `MultiPolygon`. Malta is a good small case: two members, the main island and Gozo.

```json
{
    "type": "Feature",
    "properties": {
        "NUTS_ID": "MT0",
        "LEVL_CODE": 1,
        "CNTR_CODE": "MT",
        "NUTS_NAME": "Malta"
    },
    "geometry": {
        "type": "MultiPolygon",
        "coordinates": [
            [
                [
                    [
                        14.564294854000025,
                        35.85010107000005
                    ],
                    [
                        14.508900697000058,
                        35.813378439000076
                    ],
                    [
                        14.377945005000072,
                        35.84952187400006
                    ],
                    [
                        14.335185120000062,
                        35.888628819000076
                    ],
                    [
                        14.328682825000044,
                        35.97056118300003
                    ],
                    [
                        14.377945005000072,
                        35.971847565000076
                    ],
                    [
                        14.564294854000025,
                        35.85010107000005
                    ]
                ]
            ],
            [
                [
                    [
                        14.323871851000035,
                        36.05442903900007
                    ],
                    [
                        14.265775606000034,
                        36.003346832000034
                    ],
                    [
                        14.172533043000044,
                        36.03380499700006
                    ],
                    [
                        14.169267930000046,
                        36.070509349000076
                    ],
                    [
                        14.251696669000069,
                        36.08770977700004
                    ],
                    [
                        14.323871851000035,
                        36.05442903900007
                    ]
                ]
            ]
        ]
    }
}
```

Note the winding: the first ring runs clockwise. RFC 7946 clause 3.1.6 asks a producer for the opposite, and Cassiopeia rewinds it.

## What converts on its own, and what does not

A GeoProperty holds one of six geometry types (ETSI GS CIM 009 v1.9.1 clause 4.7). When the source type is not the declared type, Cassiopeia applies three conversions without being asked, because none of them discards a coordinate:

- the source is already the declared type;
- a single geometry becomes the multi-geometry of the same kind;
- a multi-geometry carrying exactly one member becomes that member.

The third conversion matters for this dataset: 81 of the 125 regions are `MultiPolygon` values with one member, so `transformation: "polygon"` unwraps them without an explicit conversion. The remaining 44 have several members, and choosing one discards the rest. Without a `geometry` block, those 44 attributes are dropped with a warning naming the attribute and the reason, while the entity itself is still written.

Declaring the loss is what makes them convert:

```json5
location: {
    source: "{{ geometry }}",
    type: "GeoProperty",
    transformation: "polygon",
    geometry: {
        convert: "largest",
    },
},
```

`largest` keeps the member with the greatest geodesic area. For Malta, that is the main island; for Italy's `Isole`, it is Sicily. The conversion discards the other members. `first` would keep whichever member the file lists first, which is an arbitrary choice for an archipelago.

## Two GeoProperties, not one

A `near` geo-query (clause 4.10) against a boundary polygon answers a different question from one against a centroid: "which regions does this circle overlap" versus "whose centre is closest". A model that needs both should carry both instead of replacing the boundary with a pin:

```json5
centroid: {
    source: "{{ geometry }}",
    type: "GeoProperty",
    transformation: "point",
    geometry: {
        convert: "point-on-surface",
    },
},
```

`point-on-surface` is used rather than `centroid` because it is guaranteed to lie inside the region. A true centroid of a concave or archipelagic region can fall in the sea, which is a bad map pin. Both are computed in the plane, so both drift at high latitude and neither is meaningful across the antimeridian.

## Write the mapping

The complete mapping is in [region.json5](region.json5):

```json5
{
    version: "v4",
    dataModel: "Region",
    identity: {
        entityName: "{{ properties.NUTS_ID }}",
    },
    attributes: {
        name: {
            source: "{{ properties.NUTS_NAME }}",
            transformation: "string",
        },
        countryCode: {
            source: "{{ properties.CNTR_CODE }}",
            transformation: "string",
        },
        level: {
            source: "{{ properties.LEVL_CODE }}",
            transformation: "integer",
        },
        location: {
            source: "{{ geometry }}",
            type: "GeoProperty",
            transformation: "polygon",
            geometry: {
                convert: "largest",
            },
        },
        centroid: {
            source: "{{ geometry }}",
            type: "GeoProperty",
            transformation: "point",
            geometry: {
                convert: "point-on-surface",
            },
        },
    },
}
```

Both GeoProperties read the same `{{ geometry }}`; only their `transformation` and `geometry` differ.

## Run it

```bash
cassiopeia map \
    --input regions.geojson \
    --mapping region.json5 \
    --type geojson \
    --output out \
    --context none
```

The custom model needs no context or schema, so `--context none`. The run writes `out/Region.json`, one entity per region.

## Read the result

Malta, with its main island as `location` and a pin inside it as `centroid`:

```json
{
    "id": "urn:ngsi-ld:Region:MT0",
    "type": "Region",
    "name": {
        "type": "Property",
        "value": "Malta"
    },
    "countryCode": {
        "type": "Property",
        "value": "MT"
    },
    "level": {
        "type": "Property",
        "value": 1
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Polygon",
            "coordinates": [
                [
                    [
                        14.564294854000025,
                        35.85010107000005
                    ],
                    [
                        14.377945005000072,
                        35.971847565000076
                    ],
                    [
                        14.328682825000044,
                        35.97056118300003
                    ],
                    [
                        14.335185120000062,
                        35.888628819000076
                    ],
                    [
                        14.377945005000072,
                        35.84952187400006
                    ],
                    [
                        14.508900697000058,
                        35.813378439000076
                    ],
                    [
                        14.564294854000025,
                        35.85010107000005
                    ]
                ]
            ]
        }
    },
    "centroid": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                14.417046695337977,
                35.892613002000076
            ]
        }
    }
}
```

Gozo is gone, which is what `largest` was asked to do. The ring's positions are the source's, in reverse: the source wound it clockwise and Cassiopeia rewound it counterclockwise for RFC 7946 clause 3.1.6. The `centroid` at 14.417 E, 35.893 N is on the main island.

## When to convert, and when to change the model

Every conversion but the three automatic ones loses something, and a conversion in the mapping is a decision that consumers cannot see. Prefer a model that admits the source's own geometry type when you can: `transformation: "multipolygon"` here would keep every island of every region and need no `geometry` block. Convert when a published model fixes the type, when a consumer cannot handle multi-geometries, or when a derived geometry answers a question the original cannot, as the centroid does here. The [mapping guide](../../mapping.md#convert-between-geometry-types) lists every conversion, and the [`geo_convert` template function](../../templates.md#geometry-functions) reaches the same lattice for a geometry built inside a structure.
