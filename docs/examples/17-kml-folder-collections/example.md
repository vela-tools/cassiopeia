# Mapping KML folder collections in one run

A KML file can group placemarks in `<Folder>` elements. Cassiopeia exposes each folder as a **collection**, and a manifest can send each collection to its own mapping and entity type. This example uses one KML with four mapped folders and produces four NGSI-LD entity types in one run.

The [previous example](../16-kmz-folder-namespacing/example.md) had one folder, so every record shared one namespace and one mapping. Here the manifest binds several folders by name.

## Get the data

Google My Maps exports each map layer as a `<Folder>`, which makes it a convenient source for this example. Any public map can be fetched as plain KML with `forcekml=1`. This map has layers for Citi Bike docks, bike-rental shops, sights, and public restrooms:

```bash
wget -O nyc-cycling.kml "https://www.google.com/maps/d/kml?mid=1ouRmFjRHxcdgoI4HSXml8UBqaWkh1k8i&forcekml=1"
```

Replace the `mid` with your own public map's ID and the rest of the example works the same way.

## The record shape

The file holds five folders. Four contain point placemarks that this example maps; the fifth is a driving route and is left alone:

| Folder | What it holds | Mapped to |
| --- | --- | --- |
| `Citi Bike Stations` | 14 docking stations | `CitiBikeStation` |
| `Where to Get Bikes` | bike-rental shops | `BikeRentalShop` |
| `Places to Stop` | sights along the way | `Sightseeing` |
| `Restrooms` | public restrooms | `PublicRestroom` |
| `Directions from the route` | a route line | _(unmapped)_ |

As with any foldered KML, each record is nested under a lower-case, underscored version of its folder name. A placemark in `Citi Bike Stations` is under `citi_bike_stations`; one in `Where to Get Bikes` is under `where_to_get_bikes`. Each mapping uses its folder's prefix.

## Binding folders to mappings

The manifest contains the key part of the example. One input has a `mappings` list that pairs each folder's exact name with the mapping for that collection:

```json5
{
    source: "nyc-cycling.kml",
    format: "kml",
    mappings: [
        {
            collection: "Citi Bike Stations",
            mapping: "station.json5",
        },
        {
            collection: "Where to Get Bikes",
            mapping: "rental.json5",
        },
        {
            collection: "Places to Stop",
            mapping: "sight.json5",
        },
        {
            collection: "Restrooms",
            mapping: "restroom.json5",
        },
    ],
}
```

Each folder's placemarks go only to its own mapping, so one file produces four entity types. The unlisted `Directions` folder is skipped, and an unbound collection is not an error.

## Four small mappings

Each mapping reads the placemark's name and geometry through its folder namespace. Station and restroom labels include a category prefix and a non-breaking space, so an anchored strip removes the prefix while preserving the rest of the name:

```json5
// station.json5, "Citi Bike - W 15 St & 10 Ave" becomes "W 15 St & 10 Ave"
identity: {
    entityName: "{{ citi_bike_stations.properties.name | regex_replace(pattern='^Citi Bike[^0-9A-Za-z]+', rep='') | trim | clean }}",
},
attributes: {
    name: {
        source: "{{ citi_bike_stations.properties.name | regex_replace(pattern='^Citi Bike[^0-9A-Za-z]+', rep='') | trim }}",
        type: "Property",
        transformation: "string",
    },
    location: {
        source: "{{ citi_bike_stations.geometry }}",
        type: "GeoProperty",
        transformation: "geometry",
    },
},
```

The rental and sight mappings read name and location directly. One rental shop appears twice under the same name, so using the name as its identity merges the two placemarks into one entity.

## Run it

```bash
cassiopeia map --manifest manifest.json5
```

The run writes four files, one per bound folder:

```text
CitiBikeStation.json   14 entities
BikeRentalShop.json     5 entities
Sightseeing.json        7 entities
PublicRestroom.json     7 entities
```

## Read the result

Here is one entity from each mapped folder, all produced from the same file:

```json
{
    "id": "urn:ngsi-ld:CitiBikeStation:W120StClaremontAve",
    "type": "CitiBikeStation",
    "name": {
        "type": "Property",
        "value": "W 120 St & Claremont Ave"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                -73.963,
                40.811
            ]
        }
    }
}

{
    "id": "urn:ngsi-ld:BikeRentalShop:BikeRentNYC",
    "type": "BikeRentalShop",
    "name": {
        "type": "Property",
        "value": "Bike Rent NYC"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                -74.006,
                40.711
            ]
        }
    }
}

{
    "id": "urn:ngsi-ld:Sightseeing:911Memorial",
    "type": "Sightseeing",
    "name": {
        "type": "Property",
        "value": "9/11 Memorial"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                -74.013,
                40.711
            ]
        }
    }
}

{
    "id": "urn:ngsi-ld:PublicRestroom:Pier40",
    "type": "PublicRestroom",
    "name": {
        "type": "Property",
        "value": "Pier 40"
    },
    "location": {
        "type": "GeoProperty",
        "value": {
            "type": "Point",
            "coordinates": [
                -74.011,
                40.729
            ]
        }
    }
}
```

The map's folders become collections in the run: one KML file in, four entity types out.
