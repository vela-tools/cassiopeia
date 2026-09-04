# Mapping XML unit codes and observed timestamps

A measurement needs a value, a unit, and a timestamp. This example maps the latest observation from one weather station to the `WeatherObserved` Smart Data Model. Each reading gets a `unitCode` and an `observedAt`, so later fetches can add observations for the same station instead of replacing earlier readings.

## Get the data

The feed is the latest surface observation for Ljubljana Bežigrad, from the [Slovenian Environment Agency (ARSO)](https://meteo.arso.gov.si/):

```bash
wget -O observation.xml "https://meteo.arso.gov.si/uploads/probase/www/observ/surface/text/sl/observation_LJUBL-ANA_BEZIGRAD_latest.xml"
```

## Get the schema

`WeatherObserved` is a published Smart Data Model. Download the catalog once:

```bash
cassiopeia sdm download
```

## The record shape

The document contains one station inside an envelope. Feed-level fields come first, followed by one `metData` element containing the observation. Since nothing repeats, Cassiopeia keeps the document as one record and exposes the measurements under `metData`. A reading pairs a value with its unit and description:

```xml
<metData>
    <domain_id>65142878</domain_id>
    <domain_lat>46.0658</domain_lat>
    <domain_lon>14.5172</domain_lon>
    <domain_title>LJUBLJANA/BEZIGRAD</domain_title>
    <valid_UTC>21.08.2026 10:00 UTC</valid_UTC>
    <t_var_unit>°C</t_var_unit>
    <t>23</t>
    <rh>80</rh>
    <msl>1011</msl>
    <ff_val>0.2</ff_val>
    <dd_val>54</dd_val>
    <vis_value>20</vis_value>
</metData>
```

The mapping reads each field as `metData.<name>`.

## unitCode is a code, not a symbol

The feed prints units as symbols such as `°C`, `hPa`, and `m/s`. NGSI-LD's `unitCode` is different: it uses the UN/CEFACT Common Code for the unit (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2). Each field has a fixed unit, so the mapping uses a constant code:

```json5
temperature: {
    source: "{{ metData.t }}",
    type: "Property",
    transformation: "float",
    properties: {
        unitCode: {
            source: "CEL",
        },
        observedAt: {
            source: "{{ metData.valid_UTC | replace(from='.', to='/') | replace(from=' UTC', to='') }}",
            type: "Property",
            transformation: "datetime",
        },
    },
},
```

`unitCode` and `observedAt` are declared under `properties`, like other attribute-level properties. NGSI-LD reserves those names, so they are emitted as qualifiers on the value rather than as ordinary nested properties. The codes used here are:

| Quantity | Feed symbol | unitCode |
| --- | --- | --- |
| temperature, dew point | °C | `CEL` |
| relative humidity | % (as ratio) | `C62` |
| atmospheric pressure | hPa | `A97` |
| wind speed | m/s | `MTS` |
| wind direction | ° | `DD` |
| visibility | km | `KMT` |

## observedAt makes it temporal

The feed's `valid_UTC` value is `21.08.2026 10:00 UTC`, which `datetime` does not read directly. The mapping changes the dots to slashes and removes ` UTC`, producing the supported form `21/08/2026 10:00`, parsed as UTC. That instant becomes the reading's `observedAt`.

An `observedAt` makes the entity temporal. The station ID stays stable, while the timestamp keeps successive readings distinct. The [gold-price example](../07-csv-observed-at/example.md) uses the same approach for a monthly series; here each fetch adds a new observation for the station. The model also requires `dateObserved`, which comes from the same instant.

## Meeting the model

`WeatherObserved` requires `id`, `type`, `dateObserved`, and `location`. The mapping supplies the timestamp and builds a point from the station's longitude and latitude. The model expects `relativeHumidity` as a ratio in `[0, 1]`, while the feed reports a percentage, so the mapping divides it by 100. The result carries `C62`, the UN/CEFACT code for "one":

```json5
relativeHumidity: {
    source: "{{ metData.rh | int / 100 }}",
    type: "Property",
    transformation: "float",
    properties: {
        unitCode: {
            source: "C62",
        },
    },
},
```

## The manifest

```json5
{
    version: "v1",
    inputs: [
        {
            source: "observation.xml",
            mapping: "weather.json5",
            format: "xml",
        },
    ],
    output: {
        target: "file",
        directory: "out",
        context: "none",
        validation: {
            mode: "fail-when-schema",
        },
    },
}
```

## Run it

```bash
cassiopeia map --manifest manifest.json5
```

## Read the result

The output contains one `WeatherObserved` entity. Each measurement carries its unit, and the temperature also carries the time it was read:

```json
{
    "id": "urn:ngsi-ld:WeatherObserved:65142878",
    "type": "WeatherObserved",
    "stationName": {
        "type": "Property",
        "value": "LJUBLJANA/BEZIGRAD"
    },
    "dateObserved": {
        "type": "Property",
        "value": "2026-08-21T10:00:00.000Z"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                14.5172,
                46.0658
            ]
        }
    },
    "temperature": {
        "type": "Property",
        "value": 23.0,
        "observedAt": "2026-08-21T10:00:00Z",
        "unitCode": "CEL"
    },
    "relativeHumidity": {
        "type": "Property",
        "value": 0.8,
        "unitCode": "C62"
    },
    "atmosphericPressure": {
        "type": "Property",
        "value": 1011,
        "unitCode": "A97"
    },
    "windSpeed": {
        "type": "Property",
        "value": 0.2,
        "unitCode": "MTS"
    },
    "windDirection": {
        "type": "Property",
        "value": 54,
        "unitCode": "DD"
    },
    "visibility": {
        "type": "Property",
        "value": 20,
        "unitCode": "KMT"
    }
}
```

The entity passes the `WeatherObserved` schema. Its required fields are present, humidity is a ratio, and each reading states its unit and, for temperature, its observation time.
