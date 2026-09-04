# Scheduling repeated JSON feed mappings

Earlier examples run once and stop. A live feed needs a run that repeats as new readings arrive. This example uses a manifest to fetch an air-quality feed, map it to `AirQualityObserved`, upsert the sensor entities, and follow a `schedule`. It also shows what happens when a poll fails.

## Get the data

The [Sensor.Community](https://sensor.community/) network (formerly Luftdaten) publishes readings from thousands of citizen-run air-quality sensors without requiring a key. Its `/filter/area` endpoint returns a JSON array of readings from the last five minutes within a radius. This request covers 12 km around central Ljubljana:

```bash
curl -s "https://data.sensor.community/airrohr/v1/filter/area=46.05,14.51,12" -o area.json
```

Fetch it once to inspect the shape. The manifest fetches the URL on every poll. Because the feed only covers the last five minutes, a five-minute schedule picks up readings that were not present in the previous window.

## Get the schema

`AirQualityObserved` is a published Smart Data Model. Download the catalog once, so both validation and the model's `@context` can be resolved:

```bash
cassiopeia sdm download
```

## The record shape

Each array element is one reading from one sensor. A particulate sensor such as the Nova Fitness SDS011 reports two values identified by `value_type`: `P1` is PM10 and `P2` is PM2.5.

```json
{
    "timestamp": "2026-08-25 11:56:33",
    "location": {
        "longitude": "14.474",
        "latitude": "46.072",
        "altitude": "306.1",
        "country": "SI",
        "id": 30315
    },
    "id": 30475884371,
    "sensordatavalues": [
        {
            "value_type": "P1",
            "value": "30.57"
        },
        {
            "value_type": "P2",
            "value": "13.13"
        }
    ],
    "sensor": {
        "sensor_type": {
            "name": "SDS011"
        },
        "id": 44606
    }
}
```

The mapping depends on three details. Coordinates are under `location`. Measurements are an array of `{ value_type, value }` pairs, so the mapping finds a reading by code. The record has two IDs: the top-level `id` changes on every fetch, while `sensor.id` identifies the physical sensor.

## One entity per sensor

The identity is `sensor.id`, not the record's `id`:

```json5
identity: {
    entityName: "{{ sensor.id }}",
}
```

The measurement `id` changes every five minutes. Using it as the identity would create a new entity on every poll. Using `sensor.id` makes each poll update the same `urn:ngsi-ld:AirQualityObserved:<sensor.id>`. A station can have several sensors: the SDS011 above shares location `30315` with DHT22 sensor `44607`, but each sensor gets its own entity.

## Picking a reading out of the array

A measurement is not stored at a fixed key. The mapping must find the `sensordatavalues` entry with the requested `value_type`, using a template loop:

```json5
pm10: {
    source: "{% for v in sensordatavalues %}{% if v.value_type == 'P1' %}{{ v.value }}{% endif %}{% endfor %}",
    type: "Property",
    transformation: "float",
    properties: {
        unitCode: {
            source: "GQ",
        },
        observedAt: {
            source: "{{ timestamp | replace(from=' ', to='T') }}Z",
            type: "Property",
            transformation: "datetime",
        },
    },
}
```

The loop emits the value whose code matches. If a sensor does not report that code, the loop emits nothing, `float` receives an empty string, and the attribute is dropped. One mapping can therefore serve both sensor types; each entity contains only the readings its sensor provides.

## Units, humidity, and the timestamp

`GQ` is the UN/CEFACT Common Code for microgram per cubic metre, and `CEL` is the code for degrees Celsius. Humidity needs one conversion: `AirQualityObserved` expects `relativeHumidity` as a ratio in `[0, 1]`, while the feed reports a percentage. The mapping divides it by 100 with `{{ v.value | float / 100 }}` and assigns `C62`, the code for "one".

The feed timestamp is `2026-08-25 11:56:33` in UTC. The mapping changes the space to `T` and appends `Z`, producing the RFC 3339 value `2026-08-25T11:56:33Z`. It becomes `dateObserved` and each reading's `observedAt`. That qualifier lets a broker retain successive polls as history. The [weather example](../14-xml-unit-code/example.md) covers units and `observedAt` in more detail.

## The manifest and the schedule

The manifest supplies the remote source, broker destination, and schedule:

```json5
{
    version: "v1",
    inputs: [
        {
            source: "https://data.sensor.community/airrohr/v1/filter/area=46.05,14.51,12",
            mapping: "air-quality.json5",
            format: "json",
        },
    ],
    onFailure: "ignore",
    output: {
        target: "context-broker",
        url: "http://localhost:9090/",
        operation: "upsert",
        context: "default",
        validation: {
            mode: "fail-when-schema",
        },
    },
    schedule: {
        mode: "every",
        value: "5m",
        jitter: "20s",
        retry: {
            maxAttempts: 3,
            backoff: "15s",
        },
    },
}
```

The `source` is an `https` URL, so Cassiopeia fetches it on every run. The output upserts to the broker at `localhost:9090`, using Scorpio as in the [tropical-cyclone example](../22-csv-broker-temporal/example.md). Because `AirQualityObserved` is published, `context: "default"` resolves its `@context` from the downloaded catalog.

The `schedule` block controls the repeated runs:

- The trigger is defined by `mode` and `value`. `every` with a duration (`"5m"`, `"30s"`, `"2h"`) fires on an interval. The same fields support two alternatives: set `mode` to `cron` with a six-field expression for seconds-precision scheduling, or set it to `at` with an array of local times such as `08:00` and `20:00`.
- `jitter` adds a random delay of up to the specified duration before each run. That prevents many deployments polling the same feed from hitting it at exactly the same moment.
- `retry` repeats one failed run up to `maxAttempts` times, waiting `backoff` between attempts. It can ride out a dropped connection or a broker restart.

The top-level **`onFailure`** field, outside `schedule`, controls what happens after retries are exhausted. The default `abort` would stop a long-lived poller after the first failed request, so this manifest uses `onFailure: "ignore"`: the failure is logged, polling continues, and the command exits zero. `continue` also keeps polling but exits non-zero when a bounded schedule ends. See [Handle failures](../../running.md#handle-failures) for the full policy.

The first run starts immediately; later runs wait for the schedule trigger. Add `repeat` to limit the number of runs or `duration` to limit its lifetime. With neither, the process runs until interrupted.

## Run it

```bash
cassiopeia map --manifest manifest.json5
```

The first poll fetches, maps, validates, and upserts immediately. The command then stays alive and logs each later poll. Stop it with Ctrl-C. To test the schedule without a broker, use `target: "file"` and a `directory`; each run will replace the output with the latest snapshot.

## What you get

The station's two sensors become two entities. The SDS011 entity carries the particulate readings:

```json
{
    "id": "urn:ngsi-ld:AirQualityObserved:44606",
    "type": "AirQualityObserved",
    "dateObserved": {
        "type": "Property",
        "value": "2026-08-25T11:56:33.000Z"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                14.474,
                46.072
            ]
        }
    },
    "pm10": {
        "type": "Property",
        "value": 30.57,
        "observedAt": "2026-08-25T11:56:33Z",
        "unitCode": "GQ"
    },
    "pm25": {
        "type": "Property",
        "value": 13.13,
        "observedAt": "2026-08-25T11:56:33Z",
        "unitCode": "GQ"
    }
}
```

The DHT22 entity at the same location carries the climate readings and omits PM10 and PM2.5:

```json
{
    "id": "urn:ngsi-ld:AirQualityObserved:44607",
    "type": "AirQualityObserved",
    "dateObserved": {
        "type": "Property",
        "value": "2026-08-25T11:56:33.000Z"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                14.474,
                46.072
            ]
        }
    },
    "temperature": {
        "type": "Property",
        "value": 27.1,
        "observedAt": "2026-08-25T11:56:33Z",
        "unitCode": "CEL"
    },
    "relativeHumidity": {
        "type": "Property",
        "value": 0.618,
        "observedAt": "2026-08-25T11:56:33Z",
        "unitCode": "C62"
    }
}
```

Five minutes later, the schedule fetches the next window and upserts fresh readings onto the same sensor entities. Each update has a new `dateObserved` and `observedAt`. The broker keeps the latest value for each sensor, while the timestamps let its temporal history grow one poll at a time. [Output](../../output.md) covers broker delivery options, and [temporal observations](../07-csv-observed-at/example.md) explains the timestamping.
