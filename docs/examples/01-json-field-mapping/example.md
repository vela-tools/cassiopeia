# Mapping JSON fields into entities

A JSON array of chemical elements becomes NGSI-LD `ChemicalElement` entities. Because each symbol is unique, it can serve as the entity ID. Most attributes are copied unchanged; one transformation turns the atomic number from text into a number. The [next example](../02-json-id-collision/example.md) shows what happens when the obvious identity is not unique.

## Get the data

The dataset is the periodic table data from [andrejewski/periodic-table](https://github.com/andrejewski/periodic-table), one record per chemical element. It has 118 records and is licensed under the ISC license.

Download it into this directory:

```bash
wget -O element.json https://raw.githubusercontent.com/andrejewski/periodic-table/master/data.json
```

## The record shape

The generic JSON ingestor expects a top-level array of objects. It treats each object as one record. A record from this file looks like this:

```json
{
    "atomicNumber": 1,
    "symbol": "H",
    "name": "Hydrogen",
    "standardState": "gas",
    "groupBlock": "nonmetal"
}
```

The source contains more fields than the example uses. A mapping reads the fields it names and ignores the rest. Here those fields are the atomic number, symbol, name, standard state at room temperature, and periodic-table block.

## Choose the entity identity

Every record needs an ID that remains stable when the same element appears again. That choice controls which records Cassiopeia treats as one entity. Here it is straightforward: the chemical symbol is unique across the table.

```json5
identity: {
    entityName: "{{ symbol }}",
}
```

Cassiopeia combines the name with the `ChemicalElement` model to produce an ID such as `urn:ngsi-ld:ChemicalElement:H`. Since symbols do not repeat, every input record gets its own entity. If a dataset has no unique field ready to use, the identity must be assembled from several fields. That is where collisions can start, as the [next example](../02-json-id-collision/example.md) shows.

## Write the mapping

Open [element.json5](element.json5) for the full mapping:

```json5
{
    version: "v4",
    dataModel: "ChemicalElement",
    identity: {
        // The symbol is unique in this dataset, so it is enough to identify the entity.
        entityName: "{{ symbol }}",
    },
    attributes: {
        name: {
            source: "{{ name }}",
        },
        atomicNumber: {
            source: "{{ atomicNumber }}",
            // The source looks numeric, but direct copies stay text. This transformation makes it a number.
            transformation: "integer",
        },
        group: {
            source: "{{ groupBlock }}",
        },
        standardState: {
            source: "{{ standardState }}",
        },
    },
}
```

Each attribute names a target field and a `source` expression. `name`, `group`, and `standardState` have no explicit type or transformation, so they become text `Property` attributes. `group` also shows that source and target names do not have to match: it reads `groupBlock` and writes `group`.

A direct copy produces text, even when the source value looks numeric. Without a transformation, `atomicNumber` would be the string `"1"` instead of the number `1`. The `integer` transformation parses it before Cassiopeia creates the attribute. Other transformations handle decimal values (`float`), flags (`boolean`), and geometries such as `point`, used in the [next example](../02-json-id-collision/example.md).

## Run it

Run the mapping from this directory:

```bash
cassiopeia map \
    --input element.json \
    --mapping element.json5 \
    --type json \
    --output out \
    --context none
```

The command uses `--context none` so the example does not need a JSON-LD context or a Smart Data Models catalog. [Output](../../output.md#deliver-context) explains how to attach a real context later.

Cassiopeia writes one file per entity type. This run therefore creates `out/ChemicalElement.json`, a single JSON array of 118 `ChemicalElement` entities.

## Read the result

The default representation is normalized, so each attribute includes its NGSI-LD type. Entity resolution does not preserve input order. Two output entities look like this:

```json
[
    {
        "id": "urn:ngsi-ld:ChemicalElement:H",
        "type": "ChemicalElement",
        "name": {
            "type": "Property",
            "value": "Hydrogen"
        },
        "atomicNumber": {
            "type": "Property",
            "value": 1
        },
        "group": {
            "type": "Property",
            "value": "nonmetal"
        },
        "standardState": {
            "type": "Property",
            "value": "gas"
        }
    },
    {
        "id": "urn:ngsi-ld:ChemicalElement:He",
        "type": "ChemicalElement",
        "name": {
            "type": "Property",
            "value": "Helium"
        },
        "atomicNumber": {
            "type": "Property",
            "value": 2
        },
        "group": {
            "type": "Property",
            "value": "noble gas"
        },
        "standardState": {
            "type": "Property",
            "value": "gas"
        }
    }
]
```
