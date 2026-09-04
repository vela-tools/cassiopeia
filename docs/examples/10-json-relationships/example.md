# Building JSON relationships across a hierarchy

The four source files describe regions, subregions, countries, and states. The mappings create one entity type for each level and connect them with NGSI-LD relationships. Country entities also reuse the multilingual `name` from the [previous example](../08-json-language-property/example.md), along with a nested object, a structured array, and a location.

## Get the data

All four files come from [dr5hn/countries-states-cities-database](https://github.com/dr5hn/countries-states-cities-database), under the Open Database License (ODbL). There are 6 regions, 22 subregions, 250 countries, and 5308 states.

Download them into this directory:

```bash
wget -O regions.json https://raw.githubusercontent.com/dr5hn/countries-states-cities-database/master/json/regions.json
wget -O subregions.json https://raw.githubusercontent.com/dr5hn/countries-states-cities-database/master/json/subregions.json
wget -O countries.json https://raw.githubusercontent.com/dr5hn/countries-states-cities-database/master/json/countries.json
wget -O states.json https://raw.githubusercontent.com/dr5hn/countries-states-cities-database/master/json/states.json
```

## The links between the files

Each record carries its parent's ID. A subregion has a `region_id`, a country has `region_id` and `subregion_id`, and a state has `country_id`. The mappings use those foreign keys to build the relationships:

```text
Region  ←  Subregion  ←  Country  ←  State
              ↑___________/
```

## How a relationship is keyed

An NGSI-LD relationship points to another entity by URN. A `Relationship` therefore needs the target identifier and the target model. The identifier comes from `source`, and `target.entity` supplies the model:

```json5
region: {
    source: "{{ region_id }}",
    type: "Relationship",
    target: {
        entity: "Region",
    },
},
```

Cassiopeia combines them into `urn:ngsi-ld:Region:<region_id>`. The region mapping creates the same ID from `{{ id }}`, so the relationship resolves without looking up the target.

The object URN is computed from the record, so source order does not matter. A country can point to a subregion that has not been produced yet. If a foreign key is missing, Cassiopeia omits the relationship. Antarctica has no subregion ID, so its `Country` entity has a `region` link but no `subregion` link.

## Richer attribute shapes

A few richer attribute shapes appear in the country mapping. A **nested object** Property gathers three flat source fields into one structured value:

```json5
currency: {
    type: "Property",
    transformation: "object",
    mappings: {
        code: {
            source: "{{ currency }}",
            type: "Property",
            transformation: "string",
        },
        name: {
            source: "{{ currency_name }}",
            type: "Property",
            transformation: "string",
        },
        symbol: {
            source: "{{ currency_symbol }}",
            type: "Property",
            transformation: "string",
        },
    },
},
```

A **structured array** Property carries a value that is already a list of objects, such as the country's timezones. `transformation: "array"` passes the list through as one value:

```json5
timezones: {
    source: "{{ timezones }}",
    type: "Property",
    transformation: "array",
},
```

The country mapping also parses `population`, `gdp`, and `area_sq_km` as numbers, builds `location` from the coordinate pair, and copies the ISO codes, calling code, capital, and top-level domain as text.

## Write the mappings

Each source has a mapping, and each mapping carries the same multilingual `name` block. [subregion.json5](subregion.json5) is the smallest mapping that shows a relationship:

```json5
{
    version: "v4",
    dataModel: "Subregion",
    identity: {
        entityName: "{{ id }}",
    },
    attributes: {
        name: {
            type: "LanguageProperty",
            languageMap: {
                en: {
                    source: "{{ name }}",
                },
                de: {
                    source: "{{ translations.de }}",
                },
                fr: {
                    source: "{{ translations.fr }}",
                },
                es: {
                    source: "{{ translations.es }}",
                },
                ru: {
                    source: "{{ translations.ru }}",
                },
                "pt-BR": {
                    source: "{{ translations | get(key='pt-BR') }}",
                },
                "zh-CN": {
                    source: "{{ translations | get(key='zh-CN') }}",
                },
            },
        },
        region: {
            source: "{{ region_id }}",
            type: "Relationship",
            target: {
                entity: "Region",
            },
        },
        wikidataId: {
            source: "{{ wikiDataId }}",
            type: "Property",
            transformation: "string",
        },
    },
}
```

[country.json5](country.json5) adds the two relationships and richer shapes, while [region.json5](region.json5) and [state.json5](state.json5) complete the set. The `get` filter reads the hyphenated `pt-BR` and `zh-CN` tags and returns nothing for a missing translation, so the smaller subregion records still map cleanly.

## Write the manifest

One manifest binds all four sources to their mappings and shares the output:

```json5
{
    version: "v1",
    inputs: [
        {
            source: "regions.json",
            mapping: "region.json5",
            format: "json",
        },
        {
            source: "subregions.json",
            mapping: "subregion.json5",
            format: "json",
        },
        {
            source: "countries.json",
            mapping: "country.json5",
            format: "json",
        },
        {
            source: "states.json",
            mapping: "state.json5",
            format: "json",
        },
    ],
    output: {
        target: "file",
        directory: "out",
        context: "none",
    },
}
```

## Run it

```bash
cassiopeia map --manifest manifest.json5
```

The run reads all four sources and writes one file per entity type: `out/Region.json` (6), `out/Subregion.json` (22), `out/Country.json` (250), and `out/State.json` (5,308).

## Read the result

Slovenia's `Country` entity contains its multilingual name, both hierarchy relationships, the nested currency object, and the timezones array:

```json
{
    "id": "urn:ngsi-ld:Country:201",
    "type": "Country",
    "name": {
        "type": "LanguageProperty",
        "languageMap": {
            "en": "Slovenia",
            "de": "Slowenien",
            "fr": "Slovénie",
            "es": "Eslovenia",
            "ru": "Словения",
            "pt-BR": "Eslovênia",
            "zh-CN": "斯洛文尼亚"
        }
    },
    "nativeName": {
        "type": "Property",
        "value": "Slovenija"
    },
    "region": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Region:4",
        "objectType": "Region"
    },
    "subregion": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Subregion:16",
        "objectType": "Subregion"
    },
    "alpha2Code": {
        "type": "Property",
        "value": "SI"
    },
    "numericCode": {
        "type": "Property",
        "value": "705"
    },
    "capitalCity": {
        "type": "Property",
        "value": "Ljubljana"
    },
    "currency": {
        "type": "Property",
        "value": {
            "code": "EUR",
            "name": "Euro",
            "symbol": "€"
        }
    },
    "population": {
        "type": "Property",
        "value": 2130638
    },
    "timezones": {
        "type": "Property",
        "value": [
            {
                "zoneName": "Europe/Ljubljana",
                "gmtOffset": 3600,
                "gmtOffsetName": "UTC+01:00",
                "abbreviation": "CET",
                "tzName": "Central European Time"
            }
        ]
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                14.81666666,
                46.11666666
            ]
        }
    }
}
```

California's `State` entity links up to its country:

```json
{
    "id": "urn:ngsi-ld:State:1416",
    "type": "State",
    "name": {
        "type": "LanguageProperty",
        "languageMap": {
            "en": "California",
            "de": "Kalifornien",
            "fr": "Californie",
            "es": "California",
            "ru": "Калифорния",
            "pt-BR": "Califórnia",
            "zh-CN": "加利福尼亚州"
        }
    },
    "country": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Country:233",
        "objectType": "Country"
    },
    "code": {
        "type": "Property",
        "value": "US-CA"
    },
    "stateType": {
        "type": "Property",
        "value": "state"
    },
    "timezone": {
        "type": "Property",
        "value": "America/Los_Angeles"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                -118.755997,
                36.7014631
            ]
        }
    }
}
```

All 5,586 records became entities. Missing values affect only their own attributes: the three states without coordinates have no `location`, and Antarctica has no `subregion`. The rest of each entity is still written.
