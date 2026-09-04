# Reading GRIB2 fields by byte range

The [GRIB1 example](../18-grib1-derived-values/example.md) handled a regional nowcast. This one reads NOAA's [GFS](https://www.ncei.noaa.gov/products/weather-climate-models/global-forecast) model on a one-degree global grid and maps each cell to `WeatherObserved`. The mapping derives temperature, dew point, humidity, wind, pressure, visibility, and gust values from the GRIB2 fields.

By default Cassiopeia decodes GRIB2 through ecCodes, which also handles projected regional grids. A build without the C dependency can use the pure-Rust grib-rs decoder: turn off `grib2-full` with `--no-default-features` and add `--features grib1` if GRIB1 support is also needed. Either decoder works with the regular global grid, and the byte-range download below needs only `curl`.

> [!WARNING]
> Use a build with ecCodes for this example. ecCodes is enabled by the default `grib2-full` feature. The prebuilt release binaries and other `--no-default-features` builds use the pure-Rust grib-rs reader instead. It works with this regular global grid, but not with every projected grid. See [Getting started](../../getting-started.md#grib1-and-eccodes).

## Get the data

GFS is published on AWS Open Data. Each forecast file has an `.idx` sidecar listing every field and its byte offset, so a range request can fetch only the eight near-surface fields. That is about half a megabyte instead of the complete forty-megabyte file, using only `curl`:

```bash
BASE="https://noaa-gfs-bdp-pds.s3.amazonaws.com/gfs.20260823/00/atmos"
FILE="gfs.t00z.pgrb2.1p00.f000"
curl -s "$BASE/$FILE.idx" -o gfs.idx

# Keep the eight surface / 2 m / 10 m fields; each sits at a single level.
: > weather.grib2
awk -F: '
  {off[NR]=$2}
  ($4=="PRMSL" && $5=="mean sea level") ||
  ($4=="VIS"   && $5=="surface") ||
  ($4=="GUST"  && $5=="surface") ||
  ($5=="2 m above ground"  && ($4=="TMP"||$4=="DPT"||$4=="RH")) ||
  ($5=="10 m above ground" && ($4=="UGRD"||$4=="VGRD")) {keep[NR]=1}
  END{for(n=1;n<=NR;n++) if(keep[n]) print off[n]"-"(off[n+1]?off[n+1]-1:"")}' gfs.idx |
while read range; do curl -s -r "$range" "$BASE/$FILE" >> weather.grib2; done
```

Any recent cycle works; replace the date and hour in `BASE` with a current cycle. The full file contains the atmosphere at many pressure levels, so the field selection must include the level to keep upper-air data out of the surface observation.

## Get the schema

`WeatherObserved` is a published Smart Data Model. Download the catalog once:

```bash
cassiopeia sdm download
```

## The record shape

A GRIB file stores one field per parameter. Cassiopeia groups fields with the same grid, level, and time, then pivots them into one record per grid cell. Each parameter becomes a column with its canonical `snake_case` name. A cell's record looks like this:

```json
{
    "latitude": 46.0,
    "longitude": 14.0,
    "level_type": "height_above_ground",
    "level": 2,
    "referenceTime": "2026-08-23T00:00:00+00:00",
    "forecastTime": "2026-08-23T00:00:00+00:00",
    "temperature": 285.69,
    "dewpoint": 284.99,
    "humidity": 95.3
}
```

Templates read the keys directly, for example `{{ dewpoint }}`, without `this[...]`. Temperature, wind, and pressure are measured at different levels, so they arrive as separate records and a shared identity merges them into one entity, as in the [GRIB1 example](../18-grib1-derived-values/example.md#merging-two-levels-into-one-observation). The `{% if ... is defined %}` guard on each attribute ensures that a record contributes only fields it contains.

## Deriving wind from components

The model stores wind as eastward (`u`) and northward (`v`) components. `wind_speed` calculates the magnitude, and `wind_direction` calculates the meteorological bearing, the direction the wind blows _from_:

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

## Converting and rescaling the rest

The remaining fields need arithmetic or one rescaling step to produce the units that `WeatherObserved` expects:

- **Temperature and dew point** are in Kelvin; subtracting `273.15` gives Celsius (`CEL`). The `| float` keeps the value numeric before the subtraction.
- **Pressure** is in pascals; dividing by 100 gives hectopascals (`A97`).
- **Visibility** is in metres; dividing by 1000 gives kilometres (`KMT`).
- **Relative humidity** is a percentage, but the model constrains it to a ratio in `[0, 1]`. Rather than a bare division, `map_range` rescales the `[0, 100]` percentage onto `[0, 1]`, reading as the unit change it is. The ratio still carries a unit: `C62`, the UN/CEFACT code for "one":

```json5
relativeHumidity: {
    source: "{% if humidity is defined %}{{ map_range(value=humidity, in_min=0, in_max=100, out_min=0, out_max=1) }}{% endif %}",
    type: "Property",
    transformation: "float",
    properties: {
        unitCode: {
            source: "C62",
        },
    },
},
```

The wind gust (`gust`) is already a speed in metres per second and passes through directly.

## Run it

A single source file, so no manifest:

```bash
cassiopeia map \
    --input weather.grib2 \
    --mapping weather.json5 \
    --type grib \
    --output out \
    --context none \
    --validation-mode fail-when-schema
```

This writes `out/WeatherObserved.json`, 64,800 `WeatherObserved` entities, one per cell of the one-degree global grid, each carrying all eight derived quantities.

## Read the result

One cell, over Slovenia:

```json
{
    "id": "urn:ngsi-ld:WeatherObserved:u21vww43r",
    "type": "WeatherObserved",
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                14.0,
                46.0
            ]
        }
    },
    "dateObserved": {
        "type": "Property",
        "value": "2026-08-23T00:00:00.000Z"
    },
    "temperature": {
        "type": "Property",
        "value": 12.54,
        "unitCode": "CEL"
    },
    "dewPoint": {
        "type": "Property",
        "value": 11.84,
        "unitCode": "CEL"
    },
    "relativeHumidity": {
        "type": "Property",
        "value": 0.953,
        "unitCode": "C62"
    },
    "windSpeed": {
        "type": "Property",
        "value": 2.05,
        "unitCode": "MTS"
    },
    "windDirection": {
        "type": "Property",
        "value": 53,
        "unitCode": "DD"
    },
    "gustSpeed": {
        "type": "Property",
        "value": 2.0,
        "unitCode": "MTS"
    },
    "atmosphericPressure": {
        "type": "Property",
        "value": 1020.99,
        "unitCode": "A97"
    },
    "visibility": {
        "type": "Property",
        "value": 24.13,
        "unitCode": "KMT"
    }
}
```

GFS stores none of these eight output values directly; each is computed from the model state. The entity passes the `WeatherObserved` schema with its `id`, `type`, `dateObserved`, and `location`, a ratio for humidity, and a unit for each reading.
