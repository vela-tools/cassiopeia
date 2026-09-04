# Mapping CSV files with messy headers

This rainfall table from Milan combines several awkward CSV details: Latin-1 encoding, comma decimals, and long headers with degree signs, parentheses, and comparison operators. Cassiopeia detects the encoding, parses the numbers, and lets the mapping refer to those headers explicitly.

## Get the data

The dataset is [monthly precipitation 2008-2014](https://dati.comune.milano.it/dataset/ds306-ambientemeteo-precipitazioni-mese-2008-2014), published by the Comune di Milano under a Creative Commons Attribution licence. It has one row per month, 84 in all.

Download it into this directory:

```bash
wget -O precipitation.csv https://dati.comune.milano.it/dataset/b1bf9652-fe21-48c3-b0aa-9f75adc0016f/resource/9d953b9c-331e-484e-aaa2-fa70f207937d/download/ds306_ambientemeteo_precipitazioni-mese_2008-2014.csv
```

## The record shape

The file is semicolon-delimited. Its header includes the month's total, daily and hourly maxima, and several counts. Numeric values use Italian decimal commas:

```text
Anno;Mese;Totale;Massima giornaliera;Giorno di rilevamento massima giornaliera;Massima oraria;Giorno di rilevamento massima oraria;N° giorni con precipitazione;N° ore con precipitazione;N° giorni con precipitazione intensa (>= 20 mm/g);N° ore con precipitazione intensa (>= 5 mm/h)
2014;1;241,6;37,7;17;4,1;19;18;207;7;0
```

This file has two details that need special handling. It is encoded as Latin-1 (Windows-1252), so the `°` in `N°` is stored as byte `0xB0`, not as a UTF-8 sequence. Its column names also contain spaces, a degree sign, parentheses, `>=`, and a slash, so several are not valid template identifiers.

## The encoding is detected for you

Cassiopeia inspects the delimiter, quoting, and encoding before reading records. It decodes the Latin-1 file as UTF-8 during ingest, preserving the `°` in the headers. Later, the `float` transformation reads `241,6` as `241.6`.

## Addressing an awkward column

A bare `{{ Totale }}` works for a simple column name. It cannot address `N° giorni con precipitazione`, because spaces and `°` are not valid identifier characters. Use `this['...']` with the exact header instead:

```json5
rainyDays: {
    source: "{{ this['N° giorni con precipitazione'] }}",
    type: "Property",
    transformation: "integer",
},
```

The bracketed name is the real header, including its parentheses, `>=`, and other punctuation:

```json5
intenseDays: {
    source: "{{ this['N° giorni con precipitazione intensa (>= 20 mm/g)'] }}",
    type: "Property",
    transformation: "integer",
},
```

Cassiopeia preserves headers as written, so the text inside the brackets must match the file exactly.

## Write the mapping

The full mapping is in [precipitation.json5](precipitation.json5). It uses year and month for identity, parses the year, month, and counts as integers, and parses the millimetre readings as floats. `MonthlyPrecipitationObserved` is a custom type name, so there is no published schema to validate against.

```json5
{
    version: "v4",
    dataModel: "MonthlyPrecipitationObserved",
    identity: {
        entityName: "{{ Anno }}-{{ Mese }}",
    },
    attributes: {
        year: {
            source: "{{ Anno }}",
            type: "Property",
            transformation: "integer",
        },
        month: {
            source: "{{ Mese }}",
            type: "Property",
            transformation: "integer",
        },
        totalRainfall: {
            // The Italian source writes decimals with a comma, for example "241,6".
            source: "{{ Totale }}",
            type: "Property",
            transformation: "float",
        },
        maxDaily: {
            source: "{{ this['Massima giornaliera'] }}",
            type: "Property",
            transformation: "float",
        },
        rainyDays: {
            source: "{{ this['N° giorni con precipitazione'] }}",
            type: "Property",
            transformation: "integer",
        },
        rainyHours: {
            source: "{{ this['N° ore con precipitazione'] }}",
            type: "Property",
            transformation: "integer",
        },
        intenseDays: {
            source: "{{ this['N° giorni con precipitazione intensa (>= 20 mm/g)'] }}",
            type: "Property",
            transformation: "integer",
        },
        intenseHours: {
            source: "{{ this['N° ore con precipitazione intensa (>= 5 mm/h)'] }}",
            type: "Property",
            transformation: "integer",
        },
    },
}
```

## Run it

Run the mapping from this directory:

```bash
cassiopeia map \
    --input precipitation.csv \
    --mapping precipitation.json5 \
    --type csv \
    --output out \
    --context none
```

The run creates `out/MonthlyPrecipitationObserved.json`, one entity per month.

## Read the result

```json
{
    "id": "urn:ngsi-ld:MonthlyPrecipitationObserved:2014-1",
    "type": "MonthlyPrecipitationObserved",
    "year": {
        "type": "Property",
        "value": 2014
    },
    "month": {
        "type": "Property",
        "value": 1
    },
    "totalRainfall": {
        "type": "Property",
        "value": 241.6
    },
    "maxDaily": {
        "type": "Property",
        "value": 37.7
    },
    "rainyDays": {
        "type": "Property",
        "value": 18
    },
    "rainyHours": {
        "type": "Property",
        "value": 207
    },
    "intenseDays": {
        "type": "Property",
        "value": 7
    },
    "intenseHours": {
        "type": "Property",
        "value": 0
    }
}
```

The degree signs, punctuation, and Latin-1 encoding are preserved without special preprocessing. `totalRainfall` is `241.6`, parsed from `241,6`, and the counts come from headers that require bracket notation. A blank count omits its attribute, so months with no intense-rainfall hours have no value for it.
