# Mapping JSON `datasetId` instances

An NGSI-LD attribute normally has one value, but different forecast models can provide different valid values for the same day. NGSI-LD represents that as a _multi-attribute_: one attribute with several instances, each identified by `datasetId`. This example builds one from three numerical weather models and keeps those values under the shared `temperatureMax` name.

## The idea: one attribute, several instances

With more than one instance, the attribute is serialized as a JSON array instead of a single object. Each instance carries a `datasetId` URI. At most one instance may omit `datasetId`; that is the _default instance_. A consumer can request one model's view, such as ECMWF or GFS, without the model values colliding.

The specification requires each `datasetId` to be a valid URI. Cassiopeia requires a URN and applies no extra structure beyond RFC 8141. This example names each forecast source with `urn:ngsi-ld:dataset:model:<model>`.

## The data: three models, one place, ten days

[Open-Meteo](https://open-meteo.com/) serves free forecasts (CC-BY 4.0, no API key) and can return several models in one request. Each variable gets one column per model, for example `temperature_2m_max_ecmwf_ifs025`, `temperature_2m_max_gfs_global`, and `temperature_2m_max_icon_eu`. The mapping combines those columns into one logical property, `temperatureMax`, with one value per model.

The models are ECMWF IFS, NOAA GFS, and DWD ICON-EU. ICON-EU publishes only a few days ahead, so its later columns are `null`. For those days the mapping must omit the ICON-EU instance rather than emit an instance with a null value.

## Get the data

Open-Meteo returns parallel arrays under `daily`: one `time` array and one array for each model column. Cassiopeia's JSON ingestor turns a top-level array into one record per element, so the fetch uses `jq` to reshape those arrays into one object per forecast day and add the place and coordinates. Nothing is stored in the repository.

```bash
curl -s "https://api.open-meteo.com/v1/forecast?latitude=46.05&longitude=14.51&daily=temperature_2m_max,temperature_2m_min,apparent_temperature_max,apparent_temperature_min,precipitation_sum,precipitation_probability_max,wind_speed_10m_max,wind_gusts_10m_max,wind_direction_10m_dominant,shortwave_radiation_sum,relative_humidity_2m_mean,pressure_msl_mean,cloud_cover_mean&models=ecmwf_ifs025,gfs_global,icon_eu&forecast_days=10&timezone=UTC" \
    | jq '.latitude as $lat | .longitude as $lon | [.daily as $d | range(0; ($d.time | length)) as $i | ($d | to_entries | map({(.key): .value[$i]}) | add) + {place: "Ljubljana", latitude: $lat, longitude: $lon}]' \
    > forecast.json
```

The result is a ten-element array. Each element contains the date, place, coordinates, and one field for each requested model value. For example, a record has `time`, `place`, `latitude`, `longitude`, and one temperature field for each model.

## The mapping

The `instances` block contains the main pattern. The attribute declares its type, transformation, and shared properties once. `instances` then supplies one entry per model, each with its own source column and `datasetId`:

```json5
temperatureMax: {
    type: "Property",
    transformation: "float",
    properties: {
        unitCode: {
            source: "CEL",
        },
        observedAt: {
            source: "{{ time }}",
            transformation: "datetime",
        },
    },
    instances: [
        {
            source: "{{ temperature_2m_max_ecmwf_ifs025 }}",
            properties: {
                datasetId: {
                    source: "urn:ngsi-ld:dataset:model:ecmwf-ifs025",
                },
            },
        },
        {
            source: "{{ temperature_2m_max_gfs_global }}",
            properties: {
                datasetId: {
                    source: "urn:ngsi-ld:dataset:model:gfs-global",
                },
            },
        },
        {
            source: "{{ temperature_2m_max_icon_eu }}",
            properties: {
                datasetId: {
                    source: "urn:ngsi-ld:dataset:model:icon-eu",
                },
            },
        },
    ],
},
```

Attribute-level `properties` apply to every instance, so `unitCode` and `observedAt` appear on all three. Each instance adds its own `source` and `datasetId`, and can override a shared qualifier by restating it. When a model's column is `null`, as with ICON-EU after its forecast horizon, that instance resolves to nothing and is dropped.

The other forecast variables use the same pattern: `temperatureMin`, `feelsLikeTemperature*`, `precipitation`, `windSpeed`, `windGust`, `windDirection`, `relativeHumidity`, `atmosphericPressure`, and `cloudCover`. Their unit codes are `CEL`, `MMT`, `KMH`, `DD`, `A97`, and `P1`. Shortwave radiation is reported in MJ/m2, which has no Common Code, so it has no `unitCode`. `location` remains a single-instance `GeoProperty` because the place is shared by all models.

## Run it

```bash
cassiopeia map --manifest manifest.json5
```

The command reads `forecast.json`, maps each day object to a `WeatherForecast` entity, and writes `out/WeatherForecast.json`. `WeatherForecast` is a custom model, so no schema download is needed; validation checks structure only.

## The result

Each forecast variable is an array with one instance per model, and each instance has its `datasetId`:

```json
"temperatureMax": [
    {
        "type": "Property",
        "value": 22.1,
        "observedAt": "2026-08-25T00:00:00Z",
        "unitCode": "CEL",
        "datasetId": "urn:ngsi-ld:dataset:model:ecmwf-ifs025"
    },
    {
        "type": "Property",
        "value": 23.4,
        "observedAt": "2026-08-25T00:00:00Z",
        "unitCode": "CEL",
        "datasetId": "urn:ngsi-ld:dataset:model:gfs-global"
    },
    {
        "type": "Property",
        "value": 22.8,
        "observedAt": "2026-08-25T00:00:00Z",
        "unitCode": "CEL",
        "datasetId": "urn:ngsi-ld:dataset:model:icon-eu"
    }
]
```

Beyond ICON-EU's forecast horizon, the attribute has only ECMWF and GFS instances. No null instance is emitted. The three forecasts stay side by side under one property, each addressable by `datasetId`.
