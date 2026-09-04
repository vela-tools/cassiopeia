# Deriving weather values from a GRIB1 grid

GRIB stores gridded weather data as one field per parameter. Temperature is in Kelvin, while wind is split into eastward and northward components. This example maps the [Slovenian Environment Agency (ARSO)](https://meteo.arso.gov.si/) INCA nowcast to `WeatherObserved` entities, then derives Celsius, wind speed, and wind direction for every grid cell.

It reuses the coordinate-derived `geohash` identity from [keeping same-named entities apart](../02-json-id-collision/example.md) and the `unitCode` qualifier from [units and a timestamped observation](../14-xml-unit-code/example.md).

> [!WARNING]
> This example decodes GRIB1 and therefore requires ecCodes. The prebuilt release binaries do not include it (`--no-default-features`), so they cannot run this example. Install ecCodes and build from source with the default features, as described in [Getting started](../../getting-started.md#grib1-and-eccodes). ecCodes also provides the `grib_copy` and `grib_ls` tools used below.

## Get the data

GRIB1 support uses ecCodes, so install it first (`brew install eccodes`, or the equivalent package for your platform). Its command-line tools can also trim the file before mapping.

The nowcast files roll over hourly. The commands fetch the newest archive, unpack its single GRIB member, and keep only the analysis step (`+0h`, forecast step `0`) with `grib_copy`:

```bash
BASE="https://meteo.arso.gov.si/uploads/probase/www/nowcast/data"
LATEST=$(curl -s "$BASE/" | grep -oE 'nowcast_[0-9]{8}-[0-9]{4}\.zip' | head -1)
curl -s "$BASE/$LATEST" -o nowcast.zip
unzip -p nowcast.zip > nowcast_full.grb && rm nowcast.zip
grib_copy -w stepRange=0 nowcast_full.grb nowcast.grb && rm nowcast_full.grb
```

The `nowcast_[0-9]{8}-...` pattern skips the `nowcast_30min_...` files. Each archive contains one `public_inca_*.grb`. The nowcast has seven lead times, steps `0` through `6`, over the same cells. Mapping all of them would create seven readings for each cell, so `stepRange=0` keeps only the analysis for the model's reference time.

## Get the schema

`WeatherObserved` is a published Smart Data Model. Download the catalog once:

```bash
cassiopeia sdm download
```

## The record shape

`grib_ls` shows the three parameters in the trimmed file, each on its own vertical level:

```text
shortName   name                        level   typeOfLevel        stepRange
2t          2 metre temperature         2       heightAboveGround  0
10u         10 metre U wind component   10      heightAboveGround  0
10v         10 metre V wind component   10      heightAboveGround  0
```

Temperature is in Kelvin. The two wind fields are the eastward (`u`) and northward (`v`) components of the wind vector, in metres per second. All three cover the same 401 by 301 Lambert grid, or 120,701 cells.

## The wide model

A GRIB file stores one field per parameter, while an observation entity needs co-located parameters together. Cassiopeia groups fields that share a grid, level, and time, then emits one record per grid cell with one column per parameter. A cell's record looks like this:

```json
{
    "latitude": 46.0658,
    "longitude": 14.5172,
    "level_type": "height_above_ground",
    "level": 10,
    "referenceTime": "2026-08-24T10:00:00+00:00",
    "forecastTime": "2026-08-24T10:00:00+00:00",
    "wind_u": -1.42,
    "wind_v": 0.63
}
```

The parameter keys use Cassiopeia's canonical `snake_case` names, such as `wind_u`, `wind_v`, and `temperature`, so templates can read them directly without `this[...]`. The vertical level becomes `level_type` and a numeric `level`; `latitude` and `longitude` are ready to form a GeoJSON point.

## Deriving wind from components

The file stores wind components, not speed or direction. Speed is the vector's magnitude. Direction is its meteorological bearing, named for the direction the wind blows _from_. Cassiopeia provides helpers for both calculations:

```json5
windSpeed: {
    source: "{% if wind_u is defined %}{{ wind_speed(u=wind_u, v=wind_v) }}{% endif %}",
    type: "Property",
    transformation: "float",
    properties: {
        unitCode: {
            source: "MTS",
        },
    },
},
windDirection: {
    source: "{% if wind_u is defined %}{{ wind_direction(u=wind_u, v=wind_v) }}{% endif %}",
    type: "Property",
    transformation: "integer",
    properties: {
        unitCode: {
            source: "DD",
        },
    },
},
```

`wind_speed` is an alias for the `hypot(x, y)` magnitude helper. `wind_direction` uses the `bearing(east, north, convention)` helper with the `from` convention. The same helpers work for currents or any quantity represented by orthogonal components. `MTS` is the Common Code for metre per second and `DD` for degree, as in [the unit-code example](../14-xml-unit-code/example.md).

## Kelvin to Celsius

Temperature needs only one calculation: subtract `273.15` in the template to convert Kelvin to Celsius. The `| float` keeps the value numeric before the subtraction:

```json5
temperature: {
    source: "{% if temperature is defined %}{{ temperature | float - 273.15 }}{% endif %}",
    type: "Property",
    transformation: "float",
    properties: {
        unitCode: {
            source: "CEL",
        },
    },
},
```

`CEL` is the Common Code for degrees Celsius.

## Merging two levels into one observation

Temperature is measured at two metres and wind at ten. Since level identifies a grid slice, a cell's temperature and wind arrive as separate records, each with only its own parameters. Both records resolve to the same identity, the geohash of the cell's coordinates.

```json5
identity: {
    entityName: "{{ geohash(lat=latitude, lon=longitude) }}",
},
```

Records with the same identity merge into one entity, so a cell's temperature and wind records become one `WeatherObserved`. Each derived attribute has an `{% if ... is defined %}` guard. The temperature record omits wind fields, and the wind record omits temperature; after the merge, the entity contains all values from both records.

## Run it

This is a single source file, so the run needs no manifest:

```bash
cassiopeia map \
    --input nowcast.grb \
    --mapping weather.json5 \
    --type grib \
    --output out \
    --context none \
    --validation-mode fail-when-schema
```

This produces a large file: `out/WeatherObserved.json` contains 120,701 `WeatherObserved` entities, one per grid cell.

## Read the result

One cell, near Ljubljana, carries the derived quantities with their units:

```json
{
    "id": "urn:ngsi-ld:WeatherObserved:u24q465zw",
    "type": "WeatherObserved",
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                14.518358996067146,
                46.06697164994321
            ]
        }
    },
    "dateObserved": {
        "type": "Property",
        "value": "2026-08-24T10:00:00.000Z"
    },
    "temperature": {
        "type": "Property",
        "value": 25.25,
        "unitCode": "CEL"
    },
    "windSpeed": {
        "type": "Property",
        "value": 2.05,
        "unitCode": "MTS"
    },
    "windDirection": {
        "type": "Property",
        "value": 100,
        "unitCode": "DD"
    }
}
```

The GRIB file stored none of these three values directly. Temperature comes from Kelvin subtraction, while speed and direction come from `u` and `v`. The entity passes the `WeatherObserved` schema with its `id`, `type`, `dateObserved`, `location`, and unit-coded readings.
