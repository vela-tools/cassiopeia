# Adding a local `@context` to CSV entities

Earlier examples either used `context: none` or targeted a published Smart Data Model. This one defines an `ExoPlanet` model and a local JSON-LD `@context` for data from the NASA Exoplanet Archive. It also creates each planet's host star as a second entity and derives SI-coded values from the archive's astronomy units.

## Get the data

The [NASA Exoplanet Archive](https://exoplanetarchive.ipac.caltech.edu/) exposes confirmed exoplanets through a Table Access Protocol service. It accepts an SQL-like query and streams CSV, so this example needs only `curl`, not an API key or client. The query returns one row per planet and includes the host-star columns needed by the synthetic star mapping:

```bash
curl -s -G "https://exoplanetarchive.ipac.caltech.edu/TAP/sync" \
    --data-urlencode "query=select pl_name,hostname,discoverymethod,disc_year,pl_orbper,pl_rade,pl_bmasse,pl_eqt,pl_orbeccen,st_teff,st_rad,st_mass,st_spectype,sy_pnum,sy_dist,ra,dec from ps where default_flag=1" \
    --data-urlencode "format=csv" \
    -o planets.csv
```

The archive changes over time, so the download contains the current number of rows, around 6,354 planets. Each row also includes its host star's values, such as `st_teff`, `st_rad`, and `sy_dist`; the mapping can therefore build the star without another query.

## There is no Smart Data Model

The Smart Data Models catalog covers domains such as smart cities, energy, water, and agriculture, but not astronomy. There is no published `ExoPlanet` model to validate against, and `context: default` cannot resolve a context for it. This example supplies both the model name and its context locally.

A model does not have to come from the catalog. `dataModel` supplies its name, the mapping defines its attributes, and `@context` gives those local names globally meaningful terms. A custom model works like any other model in the mapping.

## The record shape

The generic CSV ingestor detects the delimiter and quoting, then uses the header as field names. A row reaches the mapping like this; an empty cell means that the archive has no measurement for that value:

```text
pl_name,hostname,discoverymethod,disc_year,pl_orbper,pl_rade,pl_bmasse,pl_eqt,pl_orbeccen,st_teff,st_rad,st_mass,st_spectype,sy_pnum,sy_dist,ra,dec
"Kepler-1513 b","Kepler-1513","Transit",2016,160.88420,8.59400,48.30991786,,0.306000,5491.00,0.95000,0.94300,"G V",2,349.24700,289.7917561,39.2852655
```

The planet name contains a space, so Cassiopeia cleans it before using it in a URN. The empty eighth column is the equilibrium temperature, which is not available for this planet. The columns from `st_teff` onward describe the host star, `Kepler-1513`, and repeat on every planet in that system.

## Author the `@context`

An NGSI-LD entity is JSON-LD, and its `@context` maps short terms such as `ExoPlanet` and `orbitalPeriod` to full IRIs. Without that mapping, `orbitalPeriod` has no shared definition; with it, two systems can resolve the same term. Since no published context exists for this model, the example provides [exoplanet-context.jsonld](exoplanet-context.jsonld):

```json
{
    "@context": {
        "exo": "https://vocab.example/exoplanet#",
        "ExoPlanet": "exo:ExoPlanet",
        "Star": "exo:Star",
        "hostStar": "exo:hostStar",
        "radiusMeters": "exo:radiusMeters",
        "massKilograms": "exo:massKilograms",
        "stellarTemperature": "exo:stellarTemperature",
        "distance": "exo:distance"
    }
}
```

The file maps terms from both models to IRIs under the invented namespace `https://vocab.example/exoplanet#`. The namespace is a documentation placeholder for wherever the vocabulary would live. The excerpt is shortened; the full file lists every attribute of both entity types.

What it does not contain is the NGSI-LD core context, the definitions of `id`, `type`, `Property`, `Relationship`, `value`, `object`, and `unitCode`. That is deliberate. In NGSI-LD the core context is always implicit: a context broker adds it to every entity on its own, with the lowest priority, so a user-supplied context that restated it would only duplicate what the broker already applies. A custom context carries the model's own terms and nothing else. The core context is assumed, not shipped.

Cassiopeia reads the file and inlines its `@context` value into every emitted entity. It does not fetch the custom namespace; consumers resolve those IRIs when they need them.

## Write the mapping

The complete mapping is in [planet.json5](planet.json5). It uses the bare model name `ExoPlanet`, reads one column for each attribute, and types the copied values. Three details are useful to examine: blank cells, the host star created alongside each planet, and unit handling.

The empty cells need **no guard**. A blank column resolves to null, and a numeric transformation drops the resulting attribute, so the entity is written without that field. This differs from the [parking example](../05-geojson-attribute-guards/example.md#drop-an-attribute-when-the-source-is-invalid), where a guard rejects the real value `-1` because the model disallows it. Here only absence needs handling. In particular, `eccentricity` can legitimately be `0`; a truthiness test such as `{% if pl_orbeccen %}` would treat that value as false and discard circular orbits. Leaving it unguarded preserves zero while still dropping blanks.

## The synthetic host star

The `hostStar` attribute does two things. It is a **relationship** to `urn:ngsi-ld:Star:<hostname>`, and its **`syntheticEntity`** creates that star during the same run from the stellar columns already present in the row:

```json5
hostStar: {
    source: "{{ hostname }}",
    type: "Relationship",
    target: {
        entity: "Star",
    },
    syntheticEntity: {
        dataModel: "Star",
        identity: {
            entityName: "{{ hostname }}",
        },
        attributes: {
            stellarTemperature: {
                source: "{{ st_teff }}",
                type: "Property",
                transformation: "float",
                properties: {
                    unitCode: {
                        source: "KEL",
                    },
                },
            },
            // ... stellarRadius, stellarMass, spectralType, numberOfPlanets,
            //     distance, rightAscension, declination
        },
    },
},
```

The input produces two entity types: the planet keeps its `hostStar` link, and the star is emitted separately. Cassiopeia applies the same URN cleaner to the planet ID, star ID, and relationship target. `Kepler-1513 b` becomes `urn:ngsi-ld:ExoPlanet:Kepler-1513b`; `HD 330075` becomes `urn:ngsi-ld:Star:HD330075` everywhere. The link and target therefore match. A star with several planets appears on several rows, but the identical same-URN star fragments merge into one entity, as in the [synthetic-entities example](../13-json-synthetic-entities/example.md).

The distance and sky coordinates describe the star, not the planet, so they belong on `Star`. The planet mapping keeps only planet-specific values.

## Units, where a code exists

The weather examples could give every reading a UN/CEFACT Common Code because their units are standard units such as degrees Celsius, metres, and hectopascals. Exoplanet data is less uniform. The orbital period uses days (`DAY`), both temperatures use kelvin (`KEL`), the sky coordinates use degrees (`DD`), and distance uses parsecs (`C63`):

```json5
distance: {
    source: "{{ sy_dist }}",
    type: "Property",
    transformation: "float",
    properties: {
        unitCode: {
            source: "C63",
        },
    },
},
```

The archive also uses Earth radii, Earth masses, solar radii, and solar masses. The UN/CEFACT list has no codes for those units, so the raw `radius`, `mass`, `stellarRadius`, and `stellarMass` attributes leave `unitCode` out. Their custom terms define the meaning and unit. Inventing a code would be misleading.

An uncoded value can still be **converted** to a coded unit. Multiplying Earth radii by Earth's mean radius gives metres (`MTR`), and multiplying Earth masses by Earth's mass gives kilograms (`KGM`). The mapping performs those calculations in the template and codes the results:

```json5
radiusMeters: {
    source: "{% if pl_rade %}{{ pl_rade | float * 6371000 }}{% endif %}",
    type: "Property",
    transformation: "float",
    properties: {
        unitCode: {
            source: "MTR",
        },
    },
},
```

The mass constant is Earth's mass, `5.972 * 10 ** 24` kg. The template number lexer does not accept exponent notation, so the `**` operator builds the `10^24` factor. The converted fields need a `{% if pl_rade %}` presence guard because the multiplication happens in the template and `float` would reject an empty cell. A radius or mass cannot be zero here, so this truthiness check is safe; that was not true for `eccentricity`.

Finally, `eccentricity` is a dimensionless ratio. It still gets `C62`, the UN/CEFACT code for "one", to make that fact explicit.

## Run it

There is one source file, so no manifest is needed. The command selects the local context file:

```bash
cassiopeia map \
    --input planets.csv \
    --mapping planet.json5 \
    --type csv \
    --output out \
    --context local \
    --context-file exoplanet-context.jsonld
```

`--context local` with `--context-file` tells Cassiopeia to read `@context` from the local JSON-LD file. It does not resolve a catalog context (`default`) or use a URL (`url`). Since the context is embedded in each entity, the run writes `out/ExoPlanet.jsonld` and `out/Star.jsonld` rather than `.json`. No catalog download or schema validation is needed for these custom models.

## Read the result

Each entity starts with the file's `@context`, followed by its cleaned `id`, `type`, and attributes. This well-populated planet, `TOI-5789 c`, carries the two SI conversions alongside the original astronomy units:

```json
{
    "@context": {
        "exo": "https://vocab.example/exoplanet#",
        "ExoPlanet": "exo:ExoPlanet",
        "hostStar": "exo:hostStar",
        "radiusMeters": "exo:radiusMeters",
        "massKilograms": "exo:massKilograms"
    },
    "id": "urn:ngsi-ld:ExoPlanet:TOI-5789c",
    "type": "ExoPlanet",
    "hostStar": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Star:TOI-5789",
        "objectType": "Star"
    },
    "discoveryMethod": {
        "type": "Property",
        "value": "Transit"
    },
    "discoveryYear": {
        "type": "Property",
        "value": 2026
    },
    "orbitalPeriod": {
        "type": "Property",
        "value": 12.927748,
        "unitCode": "DAY"
    },
    "radius": {
        "type": "Property",
        "value": 2.86
    },
    "radiusMeters": {
        "type": "Property",
        "value": 18221060.0,
        "unitCode": "MTR"
    },
    "mass": {
        "type": "Property",
        "value": 5.0
    },
    "massKilograms": {
        "type": "Property",
        "value": 2.9860000000000004e+25,
        "unitCode": "KGM"
    },
    "equilibriumTemperature": {
        "type": "Property",
        "value": 718.0,
        "unitCode": "KEL"
    },
    "eccentricity": {
        "type": "Property",
        "value": 0.067,
        "unitCode": "C62"
    }
}
```

The same run writes the host star to `Star.jsonld`, with its temperature, sky coordinates, distance in parsecs, and planet count:

```json
{
    "@context": {
        "exo": "https://vocab.example/exoplanet#",
        "Star": "exo:Star",
        "stellarTemperature": "exo:stellarTemperature",
        "distance": "exo:distance"
    },
    "id": "urn:ngsi-ld:Star:TOI-5789",
    "type": "Star",
    "stellarTemperature": {
        "type": "Property",
        "value": 5185.0,
        "unitCode": "KEL"
    },
    "stellarRadius": {
        "type": "Property",
        "value": 0.833
    },
    "stellarMass": {
        "type": "Property",
        "value": 0.821
    },
    "spectralType": {
        "type": "Property",
        "value": "K1 V"
    },
    "numberOfPlanets": {
        "type": "Property",
        "value": 4
    },
    "distance": {
        "type": "Property",
        "value": 20.4581,
        "unitCode": "C63"
    },
    "rightAscension": {
        "type": "Property",
        "value": 302.7734475,
        "unitCode": "DD"
    },
    "declination": {
        "type": "Property",
        "value": 16.1897138,
        "unitCode": "DD"
    }
}
```

The `@context` shown here is shortened to the terms used by these entities; the emitted files carry the full context on every entity. `ExoPlanet`, `Star`, and their attributes expand through the custom namespace, while `type`, `Property`, `Relationship`, and `unitCode` come from the implicit core context supplied by a broker. Coded units appear as `DAY`, `KEL`, `DD`, `C63`, `MTR`, `KGM`, and `C62`. The Earth- and solar-relative radii and masses have no standard codes, so their custom terms carry the unit meaning. Blank mass or temperature values simply omit those attributes; the entities remain valid. [Output](../../output.md#deliver-context) covers other ways to deliver a context, including a remote URL and a context broker's `Link` header.
