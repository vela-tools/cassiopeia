# Guarding GeoJSON attributes before validation

This builds on the [Smart Data Model example](../03-geojson-smart-data-model/example.md) by handling the two source fields that need conditional logic. The dataset and download steps stay the same. The mapping chooses vocabulary terms, combines a derived value with a constant in an array, and omits an attribute when its source value would fail validation.

If you have not run the [Smart Data Model example](../03-geojson-smart-data-model/example.md) yet, follow its "Get the data" and "Get the schemas" steps first; this example uses the same `parking.json` and the same downloaded catalog.

## Derive a value from a code

The source's `KATEGORIE` is a number: `1` for public parking, `2` for private, `3` for permit-only. The `OffStreetParking` model's `category` is instead an enumerated array, so a conditional maps each code to the model's word:

```json5
"{% if properties.KATEGORIE == 1 %}public{% elif properties.KATEGORIE == 2 %}private{% else %}onlyWithPermit{% endif %}"
```

The same code decides `requiredPermit`: category `3` needs a specific permit, everything else needs none.

```json5
requiredPermit: {
    source: "{% if properties.KATEGORIE == 3 %}specificIdentifiedVehiclePermit{% else %}noPermitNeeded{% endif %}",
    type: "Property",
    transformation: "array",
},
```

## Mix a derived value with a constant

`category` should record both facts about a space: its access category and that it is reserved for disabled drivers. Because the attribute is an array, its `source` is a list, and each entry is rendered on its own. The first entry is the conditional above; the second is the constant `forDisabled`:

```json5
category: {
    source: [
        "{% if properties.KATEGORIE == 1 %}public{% elif properties.KATEGORIE == 2 %}private{% else %}onlyWithPermit{% endif %}",
        "forDisabled",
    ],
    type: "Property",
    transformation: "array",
},
```

A public space therefore gets the terms `public` and `forDisabled`, while a permit-only one gets `onlyWithPermit` and `forDisabled`.

## Drop an attribute when the source is invalid

The model requires `totalSpotNumber` to be a positive integer, but the source uses `-1` for an unknown count and some rows have no count. Writing either value would fail validation. The mapping emits the number only inside a guard. For an invalid row, the guard produces an empty string, the transformation turns it into null, and Cassiopeia omits the attribute while keeping the entity:

```json5
totalSpotNumber: {
    source: "{% if properties.STELLPL_ANZ and properties.STELLPL_ANZ > 0 %}{{ properties.STELLPL_ANZ }}{% endif %}",
    type: "Property",
    transformation: "integer",
},
```

The guard has two parts. `properties.STELLPL_ANZ and ...` checks that the field exists before evaluating `... > 0`; without that short-circuit, comparing a missing value with `0` would lose the record. The second part rejects the `-1` sentinel. If either check fails, the template is empty, the integer transformation returns null, and the attribute is skipped.

## Write the mapping

The full mapping is in [parking.json5](parking.json5). It starts from the Smart Data Model example, changes `category` to a two-entry array, and adds `requiredPermit` and `totalSpotNumber`:

```json5
{
    version: "v4",
    dataModel: "dataModel.Parking/OffStreetParking",
    identity: {
        entityName: "{{ id }}",
    },
    attributes: {
        name: {
            source: "{{ properties.STRNAM }}",
            type: "Property",
            transformation: "string",
        },
        description: {
            source: "{{ properties.BESCHREIBUNG }}",
            type: "Property",
            transformation: "string",
        },
        category: {
            // Keep both values: derive the access category, then add the constant forDisabled.
            source: [
                "{% if properties.KATEGORIE == 1 %}public{% elif properties.KATEGORIE == 2 %}private{% else %}onlyWithPermit{% endif %}",
                "forDisabled",
            ],
            type: "Property",
            transformation: "array",
        },
        requiredPermit: {
            source: "{% if properties.KATEGORIE == 3 %}specificIdentifiedVehiclePermit{% else %}noPermitNeeded{% endif %}",
            type: "Property",
            transformation: "array",
        },
        allowedVehicleType: {
            source: "car",
            type: "Property",
            transformation: "array",
        },
        totalSpotNumber: {
            // The source uses -1 or an empty value when the count is unknown. The guard leaves those
            // cases empty, so the integer transformation returns null and the attribute is omitted.
            source: "{% if properties.STELLPL_ANZ and properties.STELLPL_ANZ > 0 %}{{ properties.STELLPL_ANZ }}{% endif %}",
            type: "Property",
            transformation: "integer",
        },
        openingHours: {
            source: "{{ properties.ZEITRAUM }}",
            type: "Property",
            transformation: "string",
        },
        dataProvider: {
            source: "Stadt Wien",
            type: "Property",
            transformation: "string",
        },
        source: {
            source: "https://www.data.gv.at/katalog/en/dataset/4315e096-f51e-4b56-8235-57be9789a62c",
            type: "Property",
            transformation: "string",
        },
        location: {
            source: "{{ geometry }}",
            type: "GeoProperty",
            transformation: "geometry",
        },
        address: {
            type: "Property",
            transformation: "object",
            mappings: {
                streetAddress: {
                    source: "{{ properties.STRNAM }} {{ properties.ONR_VON }}",
                    type: "Property",
                    transformation: "string",
                },
                addressLocality: {
                    source: "Wien",
                    type: "Property",
                    transformation: "string",
                },
                addressCountry: {
                    source: "Austria",
                    type: "Property",
                    transformation: "string",
                },
            },
        },
    },
}
```

## Run it

Run the mapping from this directory:

```bash
cassiopeia map \
    --input parking.json \
    --mapping parking.json5 \
    --type geojson \
    --output out \
    --context none \
    --validation-mode fail \
    --validation-representation simplified
```

All 3,854 entities pass the schema. Six of them come out without a `totalSpotNumber`, because their source count was unknown; the attribute is optional, so those entities are still valid.

## Read the result

```json
{
    "id": "urn:ngsi-ld:OffStreetParking:BEHINDERTENPARKPLATZOGD15650889",
    "type": "OffStreetParking",
    "name": {
        "type": "Property",
        "value": "Am oberen Kirchberg"
    },
    "description": {
        "type": "Property",
        "value": "Friedhof Stammersdorf-Ort"
    },
    "category": {
        "type": "Property",
        "value": [
            "public",
            "forDisabled"
        ]
    },
    "requiredPermit": {
        "type": "Property",
        "value": [
            "noPermitNeeded"
        ]
    },
    "allowedVehicleType": {
        "type": "Property",
        "value": [
            "car"
        ]
    },
    "totalSpotNumber": {
        "type": "Property",
        "value": 2
    },
    "openingHours": {
        "type": "Property",
        "value": "v. 7-19h"
    },
    "dataProvider": {
        "type": "Property",
        "value": "Stadt Wien"
    },
    "source": {
        "type": "Property",
        "value": "https://www.data.gv.at/katalog/en/dataset/4315e096-f51e-4b56-8235-57be9789a62c"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                16.411747,
                48.30367213
            ]
        }
    },
    "address": {
        "type": "Property",
        "value": {
            "streetAddress": "Am oberen Kirchberg 6",
            "addressLocality": "Wien",
            "addressCountry": "Austria"
        }
    }
}
```

`category` carries both the derived access category and the constant, `requiredPermit` follows from the same code, and `totalSpotNumber` is present because this row had a real count.
