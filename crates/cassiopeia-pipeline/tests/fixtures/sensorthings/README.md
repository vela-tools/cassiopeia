# SensorThings fixtures

The SensorThings snapshot tests in `src/end_to_end/sensorthings.rs`, `src/end_to_end/features_of_interest.rs`, `src/end_to_end/sensors.rs`, `src/end_to_end/locations.rs` and `src/end_to_end/multi_datastreams.rs` read these files from disk. The suite never contacts a server.

| File | Content |
| --- | --- |
| `rivm-v1.0-latest.jsonl` | A `latest` snapshot of RIVM Samen Meten (`https://api-samenmeten.rivm.nl/v1.0`, SensorThings API 1.0), recorded on 13 November 2026 and trimmed to two Things, `ILM_I67` (id 12511) and `ILM_I68` (id 12512), each with its `pm10`, `pm10_kal` and `pm25_kal` data streams and their newest Observations. |
| `rivm-v1.0-range.jsonl` | A `range` snapshot of RIVM Samen Meten, recorded on 13 November 2026: 6 Observations of 12 November 2026, 3 each of the `pm10` and `pm25_kal` data streams of `ILM_I68`. |
| `frost-v1.1-latest.jsonl` | A `latest` snapshot of a local FROST-Server 2.5.3 (`http://localhost:18080/FROST-Server/v1.1`, SensorThings API 1.1), recorded on 13 November 2026 from a container seeded with two Things, `eindhoven-centrum` and `helmond-west`, whose ids are texts and whose Locations are GeoJSON Features. Each has a `pm10` and a `pm25_kal` data stream with two hourly Observations. The readings are made up for the fixture. |
| `frost-v1.1-moving-range.jsonl` | A `range` snapshot in the shape a local FROST-Server 2.5.3 (`http://localhost:18080/FROST-Server/v1.1`, SensorThings API 1.1) serves with `featuresOfInterest: true`: one Thing that moves, `bus-12`, whose Location is its depot, with a `pm10` data stream of three Observations on 13 November 2026, each expanded with the FeatureOfInterest, a GeoJSON point, where the bus took it. The ids are texts. The snapshot, its places and its readings are made up for the fixture. |
| `sensors-v1.1-latest.jsonl` | A `latest` snapshot in the shape a SensorThings API 1.1 server at `https://h.example/v1.1` serves, with every Datastream expanded with its Sensor: four Things with integer ids, each with a newest Observation on 13 November 2026. Thing 1 has a `pm10` data stream from Sensor 7 (`PM-200`) and a `no2` data stream from Sensor 8 (`AQ-Multi`), Thing 2 a `pm10` data stream from Sensor 8, Thing 3 only a `no2` data stream from Sensor 8, and Thing 4 a `pm10` data stream served without a Sensor. The snapshot, its places, its Sensors and its readings are made up for the fixture. |
| `locations-v1.1-latest.jsonl` | A `latest` snapshot in the shape a SensorThings API 1.1 server at `https://h.example/v1.1` serves: two Things with integer ids, each with a `pm10` data stream with a newest Observation on 16 November 2026. Thing 1 has two Locations: a `text/plain` one (id 101) named `Stationsplein 1, Eindhoven` that holds the monitor's room, and a GeoJSON one (id 102) on the station roof. Thing 2 has one GeoJSON Location (id 201) named `Kanaaldijk 5, Helmond`. The snapshot, its places and its readings are made up for the fixture. |
| `wind-v1.1-latest.jsonl` | A `latest` snapshot in the shape a SensorThings API 1.1 server with the MultiDatastream extension at `https://h.example/v1.1` serves: Thing 1 with a `pm10` Datastream (id 1) and a wind MultiDatastream (id 4) whose ObservedProperties are `windSpeed` in `m/s` and `windDirection` in degrees, each with a newest Observation on 13 November 2026, the MultiDatastream's result `[4.2, 135]`. The snapshot, its places, its Sensors and its readings are made up for the fixture. |
| `wind-v1.1-range.jsonl` | A `range` snapshot of the same server: the wind MultiDatastream 4 with three Observations on 13 November 2026, each with a result of a wind speed and a wind direction. The readings are made up for the fixture. |
| `air-quality-observed.json5` | The `AirQualityObserved` mapping the source-format guide shows for SensorThings records. |
| `moving-air-quality-observed.json5` | The `AirQualityObserved` mapping that places each reading where it was taken, as the source-format guide shows for features of interest: its `location` is the reading's `featureOfInterest.feature`, with the reading's time as `observedAt`. |
| `device-air-quality-observed.json5` | The `AirQualityObserved` mapping that links each station to a `Device` describing the Sensor of its `pm10` reading through a `refDevice` synthetic entity, as the source-format guide shows for Sensors. |
| `addressed-air-quality-observed.json5` | The `AirQualityObserved` mapping that writes each station's `address` from the `name` of its Location with the lowest id, as the source-format guide shows for Locations, beside its `location`. |
| `wind-air-quality-observed.json5` | The `AirQualityObserved` mapping that writes a station's `pm10` Datastream reading beside the `windSpeed` and `windDirection` values of its wind MultiDatastream, each with its time as `observedAt` and its unit as `unitCode`. |
| `schemas/` | The Smart Data Models schemas the run validates against, laid out as the validator looks them up: `dataModel.Environment/AirQualityObserved.json` (schema version 0.1.4), and the documents its `$ref`s name, under the path of their published URL: `data-models/common-schema.json` and `smart-data-models/dataModel.Environment/master/Environment-schema.json`. Each is stored as published on 13 November 2026. There is no `Device` schema, so a run that writes Devices reports them as `schema-absent`. |

## How the RIVM snapshots were recorded

RIVM Samen Meten ignores nested `$expand` options written in parentheses, such as `Thing($expand=Locations)`, and answered a request that expanded every data stream's newest Observation with a gateway timeout. The snapshots were therefore recorded one data stream at a time, with requests spaced 2 seconds apart:

- each data stream with `Datastreams(<id>)?$expand=Thing/Locations,ObservedProperty`;
- its newest Observation with `Datastreams(<id>)/Observations?$orderby=phenomenonTime desc&$top=1`;
- the range with `Datastreams(<id>)/Observations?$filter=phenomenonTime ge 2026-11-12T00:00:00Z&$orderby=phenomenonTime asc&$top=3`.

The responses are written into snapshot lines unchanged: the data streams in id order, each newest Observation as its data stream's `Observations`, and the range's Observations grouped by data stream. `collectedAt` is the instant the recording started.

## How the FROST snapshot was recorded

FROST-Server 2.5.3 ran with PostgreSQL 16 and PostGIS 3.4, set with `plugins_coreModel_idType=STRING` and `persistence_idGenerationMode=ServerAndClientGenerated`, so the seeded entities keep the text ids they were created with. Text ids need the database's `uuid-ossp` extension, which FROST's database update does not create, so it was created first. The snapshot is the response to the latest discovery request, written unchanged, one Datastream per line:

```text
Datastreams?$expand=Thing($expand=Locations),ObservedProperty,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc&$top=1000
```

## Attribution

The measurements are RIVM Samen Meten's (<https://samenmeten.rivm.nl>), which RIVM, the Dutch National Institute for Public Health and the Environment, publishes as open data.

The schemas are the Smart Data Models initiative's (<https://smartdatamodels.org>), published under the [Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/) licence.
