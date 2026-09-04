# Advanced JSON schema validation for normalized output

Earlier validation examples check the **simplified (key-values)** form. There, a Property is its bare value, while metadata such as `unitCode`, `observedAt`, `datasetId`, and sub-attributes is removed. That is the usual Smart Data Model schema shape, but it cannot check metadata or less common NGSI-LD attribute types.

This example defines a `FoodProduct` model with a LanguageProperty, VocabProperty, ListProperty, and metadata-bearing Properties. It validates the **normalized** output with a hand-authored JSON Schema for the complete attribute wrapper, including values, metadata, language maps, vocabulary IRIs, and lists. The manifest supplies the schema with a per-input `schema`, following [example 27](../27-csv-custom-schema/example.md) and its command-line equivalent.

## Get the data

[Open Food Facts](https://world.openfoodfacts.org/) is a crowd-sourced food database licensed under [ODbL](https://opendatacommons.org/licenses/odbl/). The command fetches a category with a `fields=` projection, then uses `jq` to move the `products` array to the top level. JSON ingest turns that array into one record per product. The projection keeps each record small without selecting products one at a time. It uses the same `jq` approach as [example 21](../21-json-dataset-id/example.md):

```bash
curl -s -G "https://world.openfoodfacts.org/api/v2/search" \
    --data-urlencode "categories_tags_en=chocolates" \
    --data-urlencode "fields=code,product_name,product_name_en,product_name_fr,product_name_de,product_name_es,product_name_it,brands,nutriments,nutrient_levels,nutriscore_grade,nova_group,labels_tags,last_modified_t" \
    --data-urlencode "page_size=100" \
    | jq '.products' > products.json
```

The category contents can change between runs. The mapping drops empty cells, and the schema checks the _shape_ of each emitted attribute instead of requiring every possible field, so the uncurated feed can pass.

## The record shape

One projected product looks like this. Nutriment values are under `nutriments`, with keys that contain hyphens and `_100g` or `_serving` suffixes:

```json
{
    "code": "3017620422003",
    "product_name_en": "Nutella",
    "product_name_fr": "Nutella",
    "brands": "Ferrero",
    "nutriments": {
        "energy-kcal_100g": 539,
        "energy-kcal_serving": 81,
        "sugars_100g": 56.3,
        "salt_100g": 0.107
    },
    "nutrient_levels": {
        "fat": "high",
        "sugars": "high",
        "salt": "low"
    },
    "nutriscore_grade": "e",
    "nova_group": 4,
    "labels_tags": [
        "en:palm-oil",
        "en:no-gluten"
    ],
    "last_modified_t": 1705309951
}
```

## Why normalized, and why a custom schema

Smart Data Model schemas describe the simplified form and its three core types: Property, Relationship, and GeoProperty. They cannot describe a LanguageProperty's `languageMap`, a VocabProperty's `vocab`, a ListProperty's `valueList`, or a Property's `unitCode` and `observedAt`, because those details disappear in key-values. To validate them, the run uses the **normalized** representation and a schema written for it. Set `validation.representation` to `"normalized"` and provide the custom schema; the mapping does not change.

The `FoodProduct` attributes cover different parts of the normalized shape:

- `name` is a LanguageProperty. Product names in several languages become one `languageMap` keyed by BCP-47 tag (ETSI GS CIM 009 v1.9.1 clause 4.5.18). Empty translations are omitted.
- `brand` is a Relationship whose object is `urn:ngsi-ld:Brand:<brand>` (clause 4.5.3). The mapping does not create a Brand entity.
- `energy` is a Property with instances. The per-100-g and per-serving values share one attribute name and are identified by `datasetId` (clause 4.5.5). In normalized form, a multi-instance attribute is always an array, even when one instance remains.
- `sugars` is a Property with a unit and a nested VocabProperty. `unitCode` identifies grams, while the nested `level` attribute carries Open Food Facts' low/moderate/high classification. A sub-attribute keeps its own declared type (clauses 4.5.2.2 and 4.5.20), so `level` remains a VocabProperty with its classification IRI in `vocab`.
- `salt` is a Property with `observedAt`. The record's last-modified epoch becomes an ISO date-time qualifier (clause 4.8).
- `nutriScore` is a VocabProperty whose `vocab` value identifies the Nutri-Score grade (clause 4.5.20).
- `novaGroup` is an ordinary integer Property.
- `labels` is a ListProperty whose ordered tags are stored in `valueList` (clause 4.5.21).

`unitCode` values come from the `cefact-units` crate: `GRM` for gram and `E14` for kilocalorie. Cassiopeia silently drops a code it cannot parse. Where no Common Code exists, it emits no `unitCode`.

## Author the schema

The [food-product.schema.json](food-product.schema.json) schema uses draft-07 and describes each attribute's normalized wrapper. Only `id` and `type` are required. Measurements remain optional because a blank cell is omitted rather than emitted as null. The important constraints are ones a simplified schema cannot express:

```json
{
    "name": {
        "type": "object",
        "required": [
            "type",
            "languageMap"
        ],
        "properties": {
            "type": {
                "const": "LanguageProperty"
            },
            "languageMap": {
                "type": "object",
                "minProperties": 1,
                "additionalProperties": {
                    "type": "string"
                }
            }
        }
    },
    "salt": {
        "type": "object",
        "required": [
            "type",
            "value"
        ],
        "properties": {
            "type": {
                "const": "Property"
            },
            "value": {
                "type": "number"
            },
            "unitCode": {
                "enum": [
                    "GRM"
                ]
            },
            "observedAt": {
                "type": "string",
                "format": "date-time"
            }
        }
    },
    "energy": {
        "oneOf": [
            {
                "$ref": "#/definitions/energyInstance"
            },
            {
                "type": "array",
                "minItems": 1,
                "items": {
                    "$ref": "#/definitions/energyInstance"
                }
            }
        ]
    },
    "nutriScore": {
        "type": "object",
        "required": [
            "type",
            "vocab"
        ],
        "properties": {
            "type": {
                "const": "VocabProperty"
            },
            "vocab": {
                "type": "string",
                "format": "iri"
            }
        }
    },
    "sugars": {
        "type": "object",
        "required": [
            "type",
            "value"
        ],
        "properties": {
            "type": {
                "const": "Property"
            },
            "value": {
                "type": "number"
            },
            "unitCode": {
                "enum": [
                    "GRM"
                ]
            },
            "level": {
                "type": "object",
                "required": [
                    "type",
                    "vocab"
                ],
                "properties": {
                    "type": {
                        "const": "VocabProperty"
                    },
                    "vocab": {
                        "type": "string",
                        "pattern": "^https://example\\.org/nutrient-level/(low|moderate|high)$"
                    }
                }
            }
        }
    }
}
```

The `languageMap` must contain at least one string, but no particular language is required because products vary. `salt` must carry the gram code and a `date-time` in `observedAt`. `energy` accepts either one instance object or an array of instances identified by `datasetId`. `nutriScore` must use the VocabProperty wrapper and an IRI in `vocab`, and nested `sugars.level` must do the same. The full schema applies similar constraints to the remaining attributes.

## The manifest

The run is defined by the self-contained [manifest.json5](manifest.json5):

```json5
{
    version: "v1",
    inputs: [
        {
            source: "products.json",
            mapping: "food-product.json5",
            format: "json",
            schema: "food-product.schema.json",
        },
    ],
    output: {
        target: "file",
        directory: "out",
        context: "food-product-context.jsonld",
        validation: {
            mode: "fail",
            representation: "normalized",
        },
    },
}
```

Two manifest details matter. The custom schema is a **per-input `schema`** beside the input's `mapping`, never on the mapping document. `validation` is a **nested object**: `mode: "fail"` stops on a violation, and `representation: "normalized"` checks wrappers and metadata instead of key-values. These settings belong to the manifest, so `--validation-representation` cannot be combined with `--manifest`.

## Write the mapping

The [food-product.json5](food-product.json5) mapping carries no `schema` field. It only describes the record-to-entity transformation:

```json5
{
    version: "v4",
    dataModel: "FoodProduct",
    identity: {
        entityName: "{{ code }}",
    },
    attributes: {
        name: {
            type: "LanguageProperty",
            languageMap: {
                en: {
                    source: "{{ product_name_en }}",
                },
                fr: {
                    source: "{{ product_name_fr }}",
                },
                de: {
                    source: "{{ product_name_de }}",
                },
                es: {
                    source: "{{ product_name_es }}",
                },
                it: {
                    source: "{{ product_name_it }}",
                },
            },
        },
        brand: {
            source: "{{ brands }}",
            type: "Relationship",
            target: {
                entity: "Brand",
            },
        },
        energy: {
            type: "Property",
            transformation: "float",
            properties: {
                unitCode: {
                    source: "E14",
                },
            },
            instances: [
                {
                    source: "{{ nutriments | get(key='energy-kcal_100g') }}",
                    properties: {
                        datasetId: {
                            source: "urn:ngsi-ld:Dataset:per-100g",
                        },
                    },
                },
                {
                    source: "{{ nutriments | get(key='energy-kcal_serving') }}",
                    properties: {
                        datasetId: {
                            source: "urn:ngsi-ld:Dataset:per-serving",
                        },
                    },
                },
            ],
        },
        sugars: {
            source: "{{ nutriments | get(key='sugars_100g') }}",
            type: "Property",
            transformation: "float",
            properties: {
                unitCode: {
                    source: "GRM",
                },
                level: {
                    source: "https://example.org/nutrient-level/{{ nutrient_levels | get(key='sugars') }}",
                    type: "VocabProperty",
                },
            },
        },
        salt: {
            source: "{{ nutriments | get(key='salt_100g') }}",
            type: "Property",
            transformation: "float",
            properties: {
                unitCode: {
                    source: "GRM",
                },
                observedAt: {
                    source: "{{ last_modified_t | date(format='%Y-%m-%dT%H:%M:%SZ') }}",
                },
            },
        },
        nutriScore: {
            source: "https://example.org/nutriscore/{{ nutriscore_grade }}",
            type: "VocabProperty",
        },
        novaGroup: {
            source: "{{ nova_group }}",
            type: "Property",
            transformation: "integer",
        },
        labels: {
            source: "{{ labels_tags }}",
            type: "ListProperty",
            transformation: "array",
        },
    },
}
```

The nutriment keys contain hyphens, so the mapping reads them from `nutriments` with `get` instead of dotted-path syntax. The `salt` timestamp is an epoch integer; Tera's built-in `date` filter converts it to an ISO date-time for `observedAt`.

## Run it

```bash
cassiopeia map --manifest manifest.json5
```

The run writes one entity per product to `out/FoodProduct.jsonld`. The `.jsonld` extension reflects the local `@context` in each entity body. Cassiopeia validates each entity in normalized form before writing it. With `validation.mode` set to `"fail"`, a successful run confirms the wrapper and metadata constraints.

## Read the result

One normalized entity looks like this. It includes every wrapper and qualifier covered by the schema:

```json
{
    "@context": "food-product-context.jsonld (inlined)",
    "id": "urn:ngsi-ld:FoodProduct:3017620422003",
    "type": "FoodProduct",
    "name": {
        "type": "LanguageProperty",
        "languageMap": {
            "en": "Nutella",
            "fr": "Nutella"
        }
    },
    "brand": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Brand:Ferrero",
        "objectType": "Brand"
    },
    "energy": [
        {
            "type": "Property",
            "value": 539,
            "unitCode": "E14",
            "datasetId": "urn:ngsi-ld:Dataset:per-100g"
        },
        {
            "type": "Property",
            "value": 81,
            "unitCode": "E14",
            "datasetId": "urn:ngsi-ld:Dataset:per-serving"
        }
    ],
    "sugars": {
        "type": "Property",
        "value": 56.3,
        "unitCode": "GRM",
        "level": {
            "type": "VocabProperty",
            "vocab": "https://example.org/nutrient-level/high"
        }
    },
    "salt": {
        "type": "Property",
        "value": 0.107,
        "observedAt": "2024-01-15T09:12:31Z",
        "unitCode": "GRM"
    },
    "nutriScore": {
        "type": "VocabProperty",
        "vocab": "https://example.org/nutriscore/e"
    },
    "novaGroup": {
        "type": "Property",
        "value": 4
    },
    "labels": {
        "type": "ListProperty",
        "valueList": [
            "en:palm-oil",
            "en:no-gluten"
        ]
    }
}
```

A product without a per-serving energy value has a one-element `energy` array. A product without any localized name has no `name` attribute. The schema allows both cases.

## Break it on purpose

Normalized validation can catch metadata that a simplified check cannot see. Change the `unitCode` enum for `sugars` so it allows kilograms instead of the gram code emitted by the mapping:

```json
"unitCode": {
    "enum": [
        "KGM"
    ]
}
```

Rerun `cassiopeia map --manifest manifest.json5`. Products still emit `sugars` with `unitCode: "GRM"`, so validation fails. With `mode: "fail"`, the run stops before writing output. With `warn`, it completes and counts the mismatches. The [validation guide](../../validation.md#validation-modes) describes the available outcomes.

## Related pages

- [A custom model validated against its own schema](../27-csv-custom-schema/example.md), the simplified-form counterpart: a custom schema over the key-values shape.
- [A multilingual name](../08-json-language-property/example.md), the LanguageProperty in depth.
- [A controlled-vocabulary term as an IRI](../26-csv-vocab-property/example.md), the VocabProperty in depth.
- [An ordered list of values](../24-json-list-property/example.md), the ListProperty in depth.
- [A variable object kept whole](../25-geojson-json-property/example.md), another custom model built around one attribute kind.
- [Validation](../../validation.md), the modes, the representation, and the custom-schema entry points in reference form.
