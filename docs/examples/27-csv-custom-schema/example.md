# Validating two CSV custom models, each with its own schema

[Example 20](../20-csv-at-context/example.md) created an `ExoPlanet` model and local `@context`, but did not validate it because the Smart Data Models catalog has no schema for an invented model. This example supplies two local schemas and validates both models in one run: `ExoPlanet` from the planet dataset and `Star` from a separate host-star dataset.

By default, validation looks for `<EntityType>.json` in the schemas folder populated by `sdm download`. That convention works for published models, but not for custom ones. A custom schema source can be a local file or an `http(s)` URL. Supply it with `--validation-schema`, a per-input `schema` beside `mapping`, or a global `output.validation.schema`. A schema is never declared in the mapping document. This example binds one local schema to each input.

## Get the data

Both datasets are streamed as CSV from the [NASA Exoplanet Archive](https://exoplanetarchive.ipac.caltech.edu/) Table Access Protocol service, with no key or client required. The first has one row per confirmed planet:

```bash
curl -s -G "https://exoplanetarchive.ipac.caltech.edu/TAP/sync" \
    --data-urlencode "query=select pl_name,hostname,discoverymethod,disc_year,pl_orbper,pl_rade,pl_bmasse,pl_eqt,pl_orbeccen from ps where default_flag=1" \
    --data-urlencode "format=csv" \
    -o planets.csv
```

The second has one row per host star. The archive repeats stellar values on every planet row, so the query groups by `hostname` and uses `max(col)` to select one value per column. The returned hostnames are the same values used by the planets' `hostStar` relationships:

```bash
curl -s -G "https://exoplanetarchive.ipac.caltech.edu/TAP/sync" \
    --data-urlencode "query=select hostname,max(sy_pnum) sy_pnum,max(st_teff) st_teff,max(st_rad) st_rad,max(st_mass) st_mass,max(st_spectype) st_spectype,max(sy_dist) sy_dist,max(ra) ra,max(dec) dec from ps where default_flag=1 group by hostname" \
    --data-urlencode "format=csv" \
    -o stars.csv
```

The counts change as the archive changes, currently around 6,354 planets and 4,764 stars. Both files have empty cells when a quantity has not been measured.

## The two models

The distance and sky coordinates describe the star, not the planet, so they belong on `Star`. Each `ExoPlanet` keeps its own orbital and physical values and points to its host with a plain `hostStar` relationship:

```json5
hostStar: {
    source: "{{ hostname }}",
    type: "Relationship",
    target: {
        entity: "Star",
    },
},
```

Unlike [example 20](../20-csv-at-context/example.md#the-synthetic-host-star), which creates the star with a `syntheticEntity`, this run reads stars from their own dataset. The relationship supplies only the target URN; the second input creates the `Star` entity with that ID. Both mappings use `hostname` for the ID, so `HD 330075` becomes `urn:ngsi-ld:Star:HD330075` on both sides. The mappings are in [planet.json5](planet.json5) and [star.json5](star.json5).

## Author the schemas

Cassiopeia checks each NGSI-LD entity against the JSON Schema for its type. By default it validates the **simplified (key-values)** representation used by Smart Data Model schemas. The schema sees `mass` as the number `131.0`, not as the normalized Property wrapper with its `type` and `value` members. Each model has its own schema file.

[exoplanet.schema.json](exoplanet.schema.json) constrains the planet:

```json
{
    "$schema": "http://json-schema.org/draft-07/schema#",
    "title": "ExoPlanet",
    "type": "object",
    "required": [
        "id",
        "type",
        "hostStar",
        "discoveryMethod",
        "discoveryYear"
    ],
    "properties": {
        "hostStar": {
            "type": "string",
            "pattern": "^urn:ngsi-ld:Star:"
        },
        "eccentricity": {
            "type": "number",
            "minimum": 0,
            "maximum": 1
        }
    }
}
```

[star.schema.json](star.schema.json) constrains the star, with the ranges each quantity actually occupies, a positive temperature, a right ascension in `[0, 360]`, a declination in `[-90, 90]`, a positive distance:

```json
{
    "$schema": "http://json-schema.org/draft-07/schema#",
    "title": "Star",
    "type": "object",
    "required": [
        "id",
        "type",
        "numberOfPlanets"
    ],
    "properties": {
        "stellarTemperature": {
            "type": "number",
            "exclusiveMinimum": 0
        },
        "rightAscension": {
            "type": "number",
            "minimum": 0,
            "maximum": 360
        },
        "declination": {
            "type": "number",
            "minimum": -90,
            "maximum": 90
        },
        "distance": {
            "type": "number",
            "exclusiveMinimum": 0
        }
    }
}
```

The excerpts are shortened; the full files constrain every attribute of their respective models. Two schema choices are important here.

The **required set contains only fields that are always present**. The planet schema requires the ID and type that Cassiopeia emits, plus the host star, discovery method, and discovery year present in every archive row. The star schema requires the ID, type, and planet count. Physical measurements remain optional because the archive leaves many blank, and a blank cell omits the attribute rather than emitting null. Requiring `mass`, for example, would reject every planet without a measured mass.

A **key-values Relationship is a bare URN string**. In the simplified representation, `hostStar` is its target URN rather than a normalized Relationship wrapper with `type` and `object` members. The schema therefore checks a string matching `^urn:ngsi-ld:Star:`. Validating the normalized wrapper against that schema would fail for structural reasons, so this run uses key-values.

## The manifest binds each schema to its input

The run has two inputs. Each places its `schema` beside its `mapping`, binding the produced type to the correct schema:

```json5
{
    version: "v1",
    inputs: [
        {
            source: "planets.csv",
            mapping: "planet.json5",
            format: "csv",
            schema: "exoplanet.schema.json",
        },
        {
            source: "stars.csv",
            mapping: "star.json5",
            format: "csv",
            schema: "star.schema.json",
        },
    ],
    output: {
        target: "file",
        directory: "out",
        context: "none",
        validation: {
            mode: "fail",
            representation: "simplified",
        },
    },
}
```

`mode: "fail"` stops the run when an entity fails its schema. A successful run therefore confirms that every emitted entity conforms. `representation: "simplified"` selects the key-values form described by the schemas. The mapping documents carry no schema field; the manifest owns this configuration.

## Run it

```bash
cassiopeia map --manifest manifest.json5
```

The run writes `out/ExoPlanet.json` and `out/Star.json`. Planets are checked against `exoplanet.schema.json`, and stars against `star.schema.json`. No `sdm download` step is needed because the manifest names both local schemas.

## Read the result

A planet, `TOI-7510 c`, points to its host by URN:

```json
{
    "id": "urn:ngsi-ld:ExoPlanet:TOI-7510c",
    "type": "ExoPlanet",
    "hostStar": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Star:TOI-7510",
        "objectType": "Star"
    },
    "discoveryMethod": {
        "type": "Property",
        "value": "Transit"
    },
    "discoveryYear": {
        "type": "Property",
        "value": 2025
    },
    "orbitalPeriod": {
        "type": "Property",
        "value": 22.5687423,
        "unitCode": "DAY"
    },
    "radius": {
        "type": "Property",
        "value": 10.81
    },
    "mass": {
        "type": "Property",
        "value": 131.0
    },
    "equilibriumTemperature": {
        "type": "Property",
        "value": 702.0,
        "unitCode": "KEL"
    },
    "eccentricity": {
        "type": "Property",
        "value": 0.0091,
        "unitCode": "C62"
    }
}
```

The second input writes the matching star, `TOI-7510`, with its temperature, sky coordinates, distance in parsecs, and planet count:

```json
{
    "id": "urn:ngsi-ld:Star:TOI-7510",
    "type": "Star",
    "stellarTemperature": {
        "type": "Property",
        "value": 5720.0,
        "unitCode": "KEL"
    },
    "stellarRadius": {
        "type": "Property",
        "value": 1.035
    },
    "stellarMass": {
        "type": "Property",
        "value": 1.063
    },
    "spectralType": {
        "type": "Property",
        "value": "G3"
    },
    "numberOfPlanets": {
        "type": "Property",
        "value": 3
    },
    "distance": {
        "type": "Property",
        "value": 249.006,
        "unitCode": "C63"
    },
    "rightAscension": {
        "type": "Property",
        "value": 273.728188,
        "unitCode": "DD"
    },
    "declination": {
        "type": "Property",
        "value": -54.4340547,
        "unitCode": "DD"
    }
}
```

The output is normalized, so each attribute carries its `type` and, where applicable, `unitCode`. Validation used the simplified form of those same entities, where `hostStar` is a bare URN and `mass` a bare number. Both entities conform and are written.

## Break it on purpose

To see a validation failure, remove the discovery method, a required planet field, from the planet query:

```bash
curl -s -G "https://exoplanetarchive.ipac.caltech.edu/TAP/sync" \
    --data-urlencode "query=select pl_name,hostname,disc_year,pl_orbper from ps where default_flag=1" \
    --data-urlencode "format=csv" \
    -o planets-broken.csv
```

Run the planet input by itself and name the same schema with `--validation-schema`, the single-input equivalent of the per-input `schema`:

```bash
cassiopeia map \
    --input planets-broken.csv \
    --mapping planet.json5 \
    --type csv \
    --output out \
    --context none \
    --validation-mode fail \
    --validation-representation simplified \
    --validation-schema exoplanet.schema.json
```

With the column gone, Cassiopeia never produces `discoveryMethod`, so the `required` check in `exoplanet.schema.json` fails. `--validation-mode fail` aborts without writing output. With `warn`, the run completes, writes every entity, and reports each violation as a warning. The [validation guide](../../validation.md#validation-modes) describes the available outcomes.

## Naming the schema: precedence

A per-input `schema` binds the types produced by that input, just like a per-input `context`. If the run also sets `output.validation.schema`, the global source covers types that have no per-input schema:

```json5
output: {
    target: "file",
    directory: "out",
    validation: {
        mode: "fail",
        schema: "fallback.schema.json",
    },
}
```

The per-input `schema` takes precedence for its own type, and the global source supplies the rest. This allows one model to use a hand-authored schema while other types use the catalog convention or a shared fallback. `--validation-schema` is the single-input form of the same setting. A remote HTTPS schema is downloaded once at the start; a failed fetch or missing schema aborts the run. Schemas belong to the run, never to a mapping document. The [validation guide](../../validation.md#custom-schemas) covers precedence and local-versus-URL rules.

## Related pages

- [Validation](../../validation.md), the modes, the representation, and the custom-schema entry points in reference form.
- [Advanced JSON schema validation for normalized output](../28-json-advanced-schema/example.md), a custom schema that constrains the full normalized wrapper, with `unitCode`, `observedAt`, `datasetId`, and the LanguageProperty, VocabProperty, and ListProperty kinds.
- [A custom model with its own `@context`](../20-csv-at-context/example.md), the same planet model, with a synthetic host star and a local `@context` rather than a schema.
- [Data models](../../data-models.md), targeting a Smart Data Model versus an invented one, and what each gives up.
