# Mapping multilingual JSON with a LanguageProperty

Each record in this JSON array contains a region name in several languages. The mapping stores those translations in one NGSI-LD `LanguageProperty` with a `languageMap`. It also shows how the mapping chooses language tags and how the `get` filter reads a source key that dotted syntax cannot address.

## Get the data

The dataset is the region list from [dr5hn/countries-states-cities-database](https://github.com/dr5hn/countries-states-cities-database), which names each of the six world regions in English and gives translations into a spread of other languages. It has 6 records and is licensed under the Open Database License (ODbL).

Download it into this directory:

```bash
wget -O regions.json https://raw.githubusercontent.com/dr5hn/countries-states-cities-database/master/json/regions.json
```

## The record shape

The generic JSON ingestor expects a top-level array of objects and turns each object into one record. One record looks like this:

```json
{
    "id": 1,
    "name": "Africa",
    "translations": {
        "de": "Afrika",
        "fr": "Afrique",
        "es": "África",
        "it": "Africa",
        "ja": "アフリカ",
        "ko": "아프리카",
        "ru": "Африка",
        "pt-BR": "África",
        "zh-CN": "非洲"
    },
    "wikiDataId": "Q15"
}
```

The English name is in the top-level `name` field. The other translations are under `translations`, keyed by language code. The source contains more languages than the example uses; the mapping reads only the ones it names.

## The LanguageProperty and its languageMap

NGSI-LD has an attribute type for this shape. A `LanguageProperty` holds a `languageMap`, whose keys are BCP-47 language tags and whose values are the translated strings (ETSI GS CIM 009 v1.9.1 clause 4.5.18). It keeps translations together instead of scattering them across attributes such as `name_de` and `name_fr`.

The mapping sets `type: "LanguageProperty"` and supplies a `languageMap` block. Each entry pairs the output language tag with a `source` expression:

```json5
name: {
    type: "LanguageProperty",
    languageMap: {
        en: {
            source: "{{ name }}",
        },
        de: {
            source: "{{ translations.de }}",
        },
    },
},
```

The mapping chooses the tag on the left; it does not have to copy one from the source. That is why English can use the top-level `name` even though there is no `en` key under `translations`. `languageMap` keys are validated as language tags, so region-qualified tags such as `pt-BR` and `zh-CN` are valid even though they are not valid attribute names.

## Read a hyphenated source key

Two source keys, `pt-BR` and `zh-CN`, contain hyphens. A template cannot read them with a dot: `{{ translations.pt-BR }}` is parsed as `translations.pt` minus `BR`. The `get` filter looks up the exact key instead:

```json5
"pt-BR": {
    source: "{{ translations | get(key='pt-BR') }}",
},
```

The dotted form still works for keys without hyphens, so only the region-qualified tags need `get`.

## Write the mapping

The full mapping is in [region.json5](region.json5):

```json5
{
    version: "v4",
    dataModel: "Region",
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
                it: {
                    source: "{{ translations.it }}",
                },
                ja: {
                    source: "{{ translations.ja }}",
                },
                ko: {
                    source: "{{ translations.ko }}",
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
        wikidataId: {
            source: "{{ wikiDataId }}",
        },
    },
}
```

The region `id` is unique, so each of the six records becomes an entity such as `urn:ngsi-ld:Region:1`. `wikidataId` is an ordinary text Property. If a source translation is empty, its language entry is omitted instead of becoming an empty string.

## Run it

Run the mapping from this directory:

```bash
cassiopeia map \
    --input regions.json \
    --mapping region.json5 \
    --type json \
    --output out \
    --context none
```

The command uses `--context none` so the example needs no JSON-LD context or Smart Data Models catalog. Cassiopeia writes one file per entity type, so this run creates `out/Region.json`, a single JSON array of 6 `Region` entities.

## Read the result

The default representation is normalized, so each attribute carries its NGSI-LD type. A `LanguageProperty` has a `languageMap` where an ordinary Property would have one `value`. The Africa entity looks like this:

```json
{
    "id": "urn:ngsi-ld:Region:1",
    "type": "Region",
    "name": {
        "type": "LanguageProperty",
        "languageMap": {
            "en": "Africa",
            "de": "Afrika",
            "fr": "Afrique",
            "es": "África",
            "it": "Africa",
            "ja": "アフリカ",
            "ko": "아프리카",
            "ru": "Африка",
            "pt-BR": "África",
            "zh-CN": "非洲"
        }
    },
    "wikidataId": {
        "type": "Property",
        "value": "Q15"
    }
}
```

Every language named in the mapping appears under `name`, each keyed by the tag the mapping chose. The `en` entry carries the top-level `name`, while `pt-BR` and `zh-CN` resolved through the `get` filter to `África` and `非洲`.
