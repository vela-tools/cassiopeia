# Source formats

Cassiopeia converts each supported source format into a sequence of records before applying a mapping. Each format produces a different record shape, so start by checking what Cassiopeia puts in a record before writing the mapping.

Most inputs can use automatic format detection. Cassiopeia inspects the source and selects the most specific supported format it recognizes. Declare the format when the source has no useful extension, its content is ambiguous, or you want the run configuration to make the choice explicit.

The format can be set on a manifest input with `format`, or through the equivalent command-line option. The accepted values are `csv`, `json`, `geojson`, `kml`, `kmz`, `xml`, `shapefile`, `geopackage`, `grib`, and `netcdf`.

## At a glance

| Format | Records | Important source shape | Collections |
| --- | --- | --- | --- |
| CSV | One record per data row | Column names become fields | No |
| JSON | One record per object in a top-level array, or per item of a listed nested array | The object is used as-is | Nested arrays of objects become collections |
| GeoJSON | One record per feature | `id`, `properties`, `geometry`, `bbox`, and `featureCollection` | No |
| KML or KMZ | One record per placemark | `properties` and `geometry` | Folders become collections |
| XML | Determined by the repeated-child convention | Folded element tree with `@` and `#` keys | No |
| ESRI Shapefile | One record per feature | `properties` and WGS84 `geometry` | Multiple layers become collections |
| GeoPackage | One record per table row | `properties` and, for features, WGS84 `geometry` | Multiple tables become collections |
| GRIB | One record per unmasked grid cell | Coordinates, times, level, and parameter columns | No |
| NetCDF | One record per unmasked grid cell, or per station observation | Coordinates, time, level, other coordinates, and data variable columns | No |

The collection column matters when a source contains several logical groups that need different mappings. KML folders, multi-layer Shapefile archives, multi-table GeoPackages, and nested JSON arrays are currently the only formats that provide collections.

## Automatic detection and explicit format

Automatic detection uses the source content. A JSON document that is also valid GeoJSON is classified as GeoJSON rather than generic JSON. Cassiopeia checks for KML before generic XML and distinguishes a Shapefile archive from other ZIP files by its contents. It identifies GRIB from its binary marker and reads the edition from the header. It identifies NetCDF from the classic `CDF` marker and version byte, or from the HDF5 signature of a NetCDF-4 file, including one placed behind an HDF5 user block. It recognizes a GeoPackage from its SQLite header and the `application_id` it declares, including the `GP10` and `GP11` identifiers of GeoPackage 1.0 and 1.1, so a plain SQLite database is not mistaken for one.

Use an explicit format when detection does not have enough context. For example, a small or unusual delimited file may need to be declared as CSV. A source downloaded without a meaningful filename may also need an explicit format in the run configuration.

The format declaration selects the ingestor but does not change the source data. The mapping must still use the record shape that the ingestor produces.

## Inspecting a source

The `profile` command uses the same detection as a mapping run and prints the result without reading the whole source or writing anything. Use it to confirm which ingestor a file selects. For formats that expose parsing details, it also shows those details before you write a mapping or decide to declare the format explicitly.

~~~bash
cassiopeia profile energy-1m.csv
~~~

~~~text
energy-1m.csv · 127.5 MiB
CSV · text/csv · 90% confidence
delimiter "," · quote "\"" · skipRows 0 · headerRows 1 · terminator "crlf" · encoding "UTF-8"
~~~

The report has at most three lines. The first names the file and gives its size. The second shows the detection result: the format, its media type, and a confidence score. The third appears only when the format has detectable parsing details. For CSV, it shows the dialect above: the delimiter, the quoting, the preamble, the header, the line terminator, and the character encoding. It uses the member names and value syntax of a manifest input's [`dialect`](manifest.md#csv-dialect), so a member that detection got wrong can be copied into a manifest and corrected there. For GRIB, it shows the edition. Formats that need no such parameters, including JSON, GeoJSON, KML, XML, Shapefile, GeoPackage, and NetCDF, print only the first two lines.

~~~text
stations.geojson · 4.2 KiB
GeoJSON · application/geo+json · 100% confidence
~~~

The report goes to standard output, so redirection captures it intact. Cassiopeia adds colour only when the output is an interactive terminal. The report describes the format and its parsing, not the record shape. To see the fields a mapping will address, map a small sample and inspect one record.

## CSV

Cassiopeia treats a CSV source as a table. The header row supplies the field names, and each following row becomes one record. A CSV source is always a single collection, so its records do not carry a collection label.

### Dialect detection

Cassiopeia profiles the CSV before reading it. It detects the delimiter, the quote character, any preamble above the table, how many rows the header spans, and whether lines end with LF, CRLF, or a lone CR. The profiler considers comma, tab, semicolon, pipe, and space delimiters, along with double quotes, single quotes, or no quoting. A file that none of these delimiters splits into columns, such as a list of names under a `name` header, is read as a table of one column, and the profile reports it with a comma delimiter. A table of several columns always wins over a single-column reading. The ingestor reads the file with exactly the dialect the profiler reports, so the preamble it skips and the field names it uses are the ones `cassiopeia profile` shows.

A quote inside a quoted field is read as RFC 4180 writes it, doubled (`""`), unless the file escapes it with a backslash (`\"`), as MySQL's `SELECT ... INTO OUTFILE` and PHP's `fputcsv` do. Both kinds of file parse without error either way, so the profiler takes the backslash only on evidence: a quoted field that, read with doubled quotes, closes at a backslash and a quote and continues with more text, as `"say \"hi\""` does, and fewer such fields when read with the backslash. A quoted Windows path that ends in a backslash, such as `"C:\data\",1`, closes before a delimiter, so it is no such evidence and the file keeps doubled quotes. For a backslash-escaped file the profile report shows `escape "\\"`; for doubled quotes it shows no `escape`. When detection gets it wrong, state `escape` in the input's [`dialect`](manifest.md#csv-dialect).

When detection gets a member wrong, state it in the input's [`dialect`](manifest.md#csv-dialect). A stated member is used as stated, and only the members left out are detected. A dialect with every structural member stated is used without any detection.

Cassiopeia keeps a header's text as written, accented characters and punctuation included. It drops the spaces around a header, as it does around a value, so a table padded for reading, such as `id | name`, has the fields `id` and `name`. It replaces a newline inside a quoted header with a space. A field name that is not an identifier is read with bracket notation, as in `{{ this['NO2 HMW µg/m³'] }}`. A column whose header is blank is keyed by its zero-based position, as in a headerless file, so it is read as `{{ this['0'] }}`.

Two columns can end up asking for the same field name: a header repeats a name (`id;value;value`), a stacked header joins two columns to the same name, or a blank header's position equals another column's name. Cassiopeia keeps every column. One column keeps the name, and each other one is keyed by the name, an `@`, and its one-based column number, as a spreadsheet counts columns. In `id;value;value`, column 2 is read as `{{ this['value'] }}` and column 3 as `{{ this['value@3'] }}`. A named column keeps the name over a blank one, and otherwise the leftmost column keeps it. A generated name never replaces a name that another column's header already uses: if the file also has a `value@3` column, the repeat becomes `value@3@3`. The run reports one `ingest-columns-renamed` warning per file. The warning lists up to three renamed columns, each with the name its header asked for and the name to read it by, and counts the rest. With `--verbose`, it lists every renamed column.

> [!WARNING]
> Migrating from v1.1.1: in a header that repeats a name, the last column used to keep the name and the earlier ones were dropped. Now the leftmost column keeps it, so in `id;value;value` a mapping's `{{ this['value'] }}` reads column 2 instead of column 3. To keep reading column 3, change the mapping to `{{ this['value@3'] }}`.

### Character encoding

Cassiopeia reads a CSV in the character encoding it detects and converts it to UTF-8, so a Windows-1252 or ISO-8859-2 file is read correctly rather than rejected. A file that starts with a byte order mark is read as the UTF-8, UTF-16LE, or UTF-16BE that the mark names, ahead of anything else, and `cassiopeia profile` reports that encoding. A file without one is read as UTF-16, as in a spreadsheet's Unicode text export, when its byte count is even and, in one byte order, every character it holds below U+0100 is text: a printable character, a tab, or a line end, never a NUL or another control character. The delimiters and line ends of a CSV are such characters, so a file of long Chinese, Japanese, or Korean cells is recognised as surely as one of ASCII. When both byte orders read as text, the one with more line ends wins. A `charset` the server declares wins over this check. For a source fetched by URL, a `charset` the server declares in its `Content-Type` decides the encoding outright. A `charset` such as `iso-2022-kr`, which the Encoding Standard decodes to a single replacement character, declares nothing. Without a `charset`, the encoding is guessed from the content, and the domain the file is served from, after any redirect, weighs the guess toward the encodings used in that region, as a browser does for an undeclared page. That settles files whose only non-ASCII characters are symbols. In Windows-1252, `µg/m³` and `°C` use bytes that ISO-8859-2 reads as the letters `ľ` and `ł`, and content alone cannot tell which was meant. Served from a `.at` or `.de` domain, such a file reads as Windows-1252. A local file has no such context, so its encoding comes from its content alone, and `cassiopeia profile` on a downloaded copy can report a different encoding than a run that reads the same file by URL. An `encoding` stated in the input's `dialect` wins over both the declared `charset` and the guess, but not over a byte order mark: the file is always read in the encoding its mark names, so the run uses and reports that encoding.

The guess weighs the whole file, not only the rows the dialect is detected from, so a name with an accented letter far down a file of plain ASCII rows still decides the encoding. Only lines with a non-ASCII character carry evidence, so only those are weighed. Up to 8 MiB of them is drawn from across the whole file, which keeps detection under a second for a file of any size. A file that is valid UTF-8 throughout is read as UTF-8. A file whose lines end with a lone carriage return is weighed line by line too. A file is never repaired. When a line holds bytes that are not valid in the encoding the file is read as, typically because a server declared `charset=utf-8` for a Windows-1252 file, the run fails and names the line of the file and the encoding, counting from the first line, title and header included. This is a line of the file, as a text editor numbers it, and not the row a misshapen-row warning names: every line break counts here, the one inside a quoted field and a blank line included. The bytes are never replaced with `�` in the values Cassiopeia maps.

### Preamble and stacked headers

Some feeds put a title line above the table. Cassiopeia skips such leading rows as a preamble when each of them has a different number of fields from the table below. A title row with as many fields as the table is also skipped in two cases. The first is a title padded to the table's width, with text in its first cell at most and the rest blank, as spreadsheet exports write it (`Air quality report;;;`); a header whose only label follows a blank first cell, as pandas writes an unnamed index column (`;Mayor`), is a header and not such a title; a run of such rows, blank rows included, directly above the header is skipped whole. The second is a fully filled title above a header whose labels sit over at least two numeric, date, or other typed columns, where the header row reads as a header and not as data. A missing-value marker over a typed column, such as `n/a`, `NULL`, `NaN`, or `NE`, counts as neither a label nor a value, so a first data row of such markers never turns the real header above it into a title. A fully filled title above a table whose columns are all text cannot be told apart from a header and is read as one. The preamble is counted in CSV records, not lines, so a quoted title that spans two lines is one record and the header below it is read intact. A blank line is not a record, so a title over a blank line over the header is a preamble of one. The profile report shows it as `skipRows 1`. Detection looks for a preamble in the first 10 records only, so a file with a longer one needs `skipRows` stated in the input's [`dialect`](manifest.md#csv-dialect).

A header can also span several rows, such as a name row, an aggregation row, and a unit row. Cassiopeia treats a row directly below the header as part of it when every non-blank cell is a short label with no number or date in it, and the row leaves its first column blank while every data row fills it, as a stacked header leaves its station or key column unlabelled. A first data row with a blank cell further along, a missing value, still fills its key column, so it stays data. Each column's non-blank header cells are joined from top to bottom with a space, so `NO2` over `HMW` over `µg/m³` becomes the field `NO2 HMW µg/m³`. Two columns that share a name but differ in a lower row, such as `PM10 MW24 µg/m³` and `PM10 HMW µg/m³`, become distinct fields. The profile report shows the header as `headerRows 3`. A header spans at most four rows. A lower header row with another number of fields than the first, such as a unit row one field short, is joined over the columns it reaches rather than failing the file.

### Rows of another width

RFC 4180 has every row of a CSV hold the same number of fields. A row that does not, such as a row one field short, a row with an extra field, or a `Source: …` note below the table, is skipped. Which of its fields is missing or extra cannot be told, so it is never padded or cut to fit. The rows around it are read as usual. A trailing delimiter is the one exception, since the field it adds is empty and holds no value. A row that ends with a delimiter its table's header does not have, such as `1;2;3;` under `a;b;c`, is read with that empty field dropped. A header that ends with one, such as `a;b;c;` over `1;2;3`, gives the table an unnamed last column, which rows without the delimiter leave empty. Either way only the empty fields at the end of a row are dropped or added, and a row short of a named column, or with a value past the table's last column, is still skipped. A file without a header follows the same rule, with its first row in place of the header. The run reports one `ingest-rows-misshapen` warning per file, naming a downloaded file by its URL. The warning names the table's field count and up to three skipped rows with their field counts, and counts the rest. A row is numbered from the top of the file, title and header rows included, as a spreadsheet numbers rows. A quoted field that spans several lines leaves its row one row, and a blank line is not counted. A row number can therefore be lower than the line a text editor shows, which is the number an encoding error names. Detection skips such rows too, as long as most rows of the sample have the table's width.

> [!WARNING]
> A footer row as wide as the table, such as a `Total;;60` sum row, cannot be told apart from data, so it is mapped as a record. Remove such rows before ingesting.

### Value inference

Cassiopeia converts CSV fields to JSON values as it reads them:

- Empty fields become `null`, and so does `\N`, the missing-value token of SQL dumps.
- Values listed in the input's `dialect.nullValues`, such as `NE`, become `null`.
- Integers and decimal numbers become JSON numbers. A decimal comma, as in `12,45`, is read as a number only when the input's `dialect` states `decimalSeparator: ","`; otherwise the value stays a string.
- `true`, `yes`, and `on` become `true`; `false`, `no`, and `off` become `false`, ignoring case.
- Values with leading zeroes remain strings, which preserves identifiers such as `00123`.
- Everything else remains a string.

This inference provides a useful starting point, but it does not replace an intentional mapping. Use an explicit transformation when the target model requires a particular type or when the value's meaning is not clear from its spelling.

### Mapping a CSV record

For a row such as:

~~~text
station_id,name,temperature
A-17,Main station,21.5
~~~

The mapping can address the fields directly:

~~~json5
identity: {
    entityName: "Station-{{ station_id }}",
},
attributes: {
    name: {
        source: "{{ name }}",
    },
    temperature: {
        source: "{{ temperature }}",
        transformation: "float",
    },
}
~~~

Mappings address named fields, so a headerless file may need preprocessing before it can be mapped cleanly. If Cassiopeia cannot infer the dialect reliably, declare `format: "csv"`, state the members it gets wrong in the input's [`dialect`](manifest.md#csv-dialect), and verify the resulting record shape before writing the mapping.

## JSON

The generic JSON ingestor decides the records from the shape of the document. It applies the same rule to every document and never guesses from member names.

An array of objects is the most common shape. Each object becomes one record, and its nested structure is preserved:

~~~json
[
    {
        "id": "A-17",
        "reading": {
            "value": 21.5
        }
    },
    {
        "id": "A-18",
        "reading": {
            "value": 20.9
        }
    }
]
~~~

A mapping can address the nested value with `{{ reading.value }}`.

A single object is one record:

~~~json
{
    "id": "A-17",
    "reading": {
        "value": 21.5
    }
}
~~~

An object with exactly one array member is an envelope. The items of that array are the records, and the other members of the object are dropped. This document yields the records `A-17` and `A-18`, and `generated` is part of neither:

~~~json
{
    "generated": "2026-10-25",
    "items": [
        {
            "id": "A-17"
        },
        {
            "id": "A-18"
        }
    ]
}
~~~

An envelope whose array is empty yields no records.

An object with no array member, or with two or more, is one record. Cassiopeia does not pick one array over another, so this document is a single record that keeps both arrays:

~~~json
{
    "id": "A-17",
    "inbound": [
        {
            "line": "1"
        }
    ],
    "outbound": [
        {
            "line": "2"
        }
    ]
}
~~~

Every record must be a JSON object. A top-level scalar is refused with `expected a top-level JSON object or an array of objects`. An array, or an envelope array, that holds anything other than objects is refused with `expected every array element to be a JSON object`.

The envelope rule limits what a single record can hold. A single object with exactly one array member is read as an envelope, not as a record. When that array holds objects, they become the records and the other members are lost:

~~~json
{
    "id": "C-1",
    "tags": [
        {
            "id": "x"
        }
    ]
}
~~~

This document yields one record, `{"id": "x"}`, and `C-1` is lost. When the array holds scalars, as in `"tags": ["a", "b"]`, the document is refused. Wrap such a record in an array, `[{...}]`, so that the array is the top-level value and the record is read whole.

Generic JSON and GeoJSON use the same syntax, but they have different record conventions. Cassiopeia routes a valid GeoJSON document to the GeoJSON ingestor.

### Nested collections

The rules above apply to an input with a single `mapping`. An input with a `mappings` list reads the JSON document as collections instead, which reaches records that sit below the top level.

Every non-empty array whose items are all objects is a collection. Its label is the path of member names that leads to it from the root, joined with `.` and without array indices. The root array has no member path, so it is not a collection itself, but the members of its items are. An array that holds anything other than objects is a plain value. This document has the collections `data.monitors`, `data.monitors.lines`, and `data.monitors.lines.departures.departure`:

~~~json
{
    "data": {
        "monitors": [
            {
                "locationStop": {
                    "properties": {
                        "title": "Schwedenplatz",
                        "attributes": {
                            "rbl": 4116
                        }
                    }
                },
                "lines": [
                    {
                        "name": "U1",
                        "towards": "Oberlaa",
                        "departures": {
                            "departure": [
                                {
                                    "departureTime": {
                                        "countdown": 5
                                    }
                                }
                            ]
                        }
                    }
                ]
            }
        ]
    },
    "message": {
        "value": "OK"
    }
}
~~~

Each item of a listed collection becomes one record. A collection that is not listed yields no records, and the members outside every listed collection, such as `message` above, are dropped. The input below maps one record per departure:

~~~json5
{
    source: "https://www.wienerlinien.at/ogd_realtime/monitor?stopId=4116",
    format: "json",
    mappings: [
        {
            collection: "data.monitors.lines.departures.departure",
            mapping: "departure.json5",
        },
    ],
}
~~~

A record keeps its own fields whole, nested collections included. It also carries every object that encloses it on the path from the root, whether an array item or a plain object, under the name of the member that holds it. An enclosing object comes without its collections and without the members on the listed path, because those are records or enclosing objects of their own. An enclosing object left with no fields is not added, which is why the record below has no `data` and no `departures`. The departure record above is:

~~~json
{
    "departureTime": {
        "countdown": 5
    },
    "lines": {
        "name": "U1",
        "towards": "Oberlaa"
    },
    "monitors": {
        "locationStop": {
            "properties": {
                "title": "Schwedenplatz",
                "attributes": {
                    "rbl": 4116
                }
            }
        }
    }
}
~~~

Its mapping can read `{{ departureTime.countdown }}`, `{{ lines.name }}`, and `{{ monitors.locationStop.properties.attributes.rbl }}`. When a name clashes, the record's own field wins, and then the nearest enclosing item. The items of a root array enclose nothing, because the root array has no name to file them under.

A lone object on a listed path counts as a collection of one item. JSON converted from XML writes an element that occurs once as an object and the same element occurring twice as an array, so the shape of a collection changes with the number of its items. In this document, Barking has two sites and Hackney has one, and `Index.Authority.Site.Species` still yields one record per reading, each carrying its `Site` and its `Authority`:

~~~json
{
    "Index": {
        "Authority": [
            {
                "@Name": "Barking",
                "Site": [
                    {
                        "@Code": "BG1",
                        "Species": [
                            {
                                "@Code": "NO2",
                                "@Index": "1"
                            }
                        ]
                    },
                    {
                        "@Code": "BG2",
                        "Species": {
                            "@Code": "NO2",
                            "@Index": "3"
                        }
                    }
                ]
            },
            {
                "@Name": "Hackney",
                "Site": {
                    "@Code": "HK6",
                    "Species": [
                        {
                            "@Code": "NO2",
                            "@Index": "4"
                        }
                    ]
                }
            }
        ]
    }
}
~~~

An object that is not on a listed path stays an ordinary field of the object that holds it.

Listing both a collection and one nested in it yields records from both, in document order. A label that matches no collection yields no records. A top-level scalar is refused as it is under a single `mapping`.

## GeoJSON

Cassiopeia supports GeoJSON `FeatureCollection` documents and single GeoJSON `Feature` documents. Each feature becomes one record. When the corresponding source members are present, the record has this shape:

~~~json
{
    "id": "A-17",
    "properties": {
        "name": "Main station"
    },
    "geometry": {
        "type": "Point",
        "coordinates": [
            14.51,
            46.05
        ]
    },
    "bbox": [
        14.50,
        46.04,
        14.52,
        46.06
    ]
}
~~~

Cassiopeia copies the feature ID to `id`, copies `properties` as an object, and copies `geometry` as a GeoJSON geometry. It also copies `bbox` when present. A feature of a `FeatureCollection` also carries its collection's members under `featureCollection` (see [Collection members](#collection-members)). A geometry-only GeoJSON document is not accepted as a source collection.

RFC 7946 lets a feature's geometry be a `GeometryCollection`, which NGSI-LD does not admit as a GeoProperty value. Such a feature produces no GeoProperty: the attribute is dropped and the run counts a warning, unless the mapping sets `geometry.convert` to `flatten` to fold the collection into one geometry. A plain `Property` with `transformation: "geometry"` keeps the collection as its GeoJSON object instead (see [GeoProperty](mapping.md#geoproperty)).

The source geometry is ready to use as an NGSI-LD `GeoProperty`, which keeps the geometry as read without a transformation:

~~~json5
location: {
    type: "GeoProperty",
    source: "{{ geometry }}",
}
~~~

Properties remain under `properties`, so a source field called `name` is addressed as `{{ properties.name }}`. Cassiopeia does not automatically use the feature ID as the NGSI-LD entity ID. Use it explicitly in `identity.entityName` when you want that identity.

### Collection members

Some sources state a value once for the whole `FeatureCollection` instead of repeating it on every feature, such as an observation time shared by every reading. RFC 7946 section 6.1 calls such members foreign members. Cassiopeia gives every feature's record its collection's foreign members and its `bbox`, when present, under `featureCollection`, much as a KML placemark carries the folder that encloses it. The collection's `type` is always `FeatureCollection` and its `features` are the records themselves, so neither is copied. A feature's own `bbox` stays at the top level of its record, beside the collection's.

This collection states its observation time once, in `timestamps`:

~~~json
{
    "type": "FeatureCollection",
    "timestamps": ["2026-10-25T07:50+00:00"],
    "features": [
        {
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [16.37, 48.2]},
            "properties": {"station": "11034", "parameters": {"TL": {"data": [6.9]}}}
        }
    ]
}
~~~

Each reading then takes its observation time from the collection:

~~~json5
temperature: {
    source: "{{ properties.parameters.TL.data | first }}",
    transformation: "float",
    properties: {
        observedAt: {
            source: "{{ featureCollection.timestamps | first }}",
            transformation: "datetime",
        },
    },
}
~~~

A collection with no foreign members and no `bbox` gives its features an empty `featureCollection`. A single `Feature` document has no collection, so its record has no `featureCollection`, and a template that reads it resolves to nothing, like any missing field.

## KML and KMZ

KML and KMZ are geospatial document formats, with KMZ being the zipped form of KML. Cassiopeia walks nested `Document` and `Folder` structures and emits one record for each `Placemark`.

A placemark record has the same general shape as a GeoJSON feature. This placemark sits in a folder named `Stations`, so its data is nested under the folder's snake-case name, `stations` (see [Folders and collections](#folders-and-collections)):

~~~json
{
    "stations": {
        "id": "station-17",
        "properties": {
            "folder": "Stations",
            "name": "Main station",
            "description": "Central platform",
            "timeStamp": "2026-08-03T10:00:00Z",
            "temperature": "21.5"
        },
        "geometry": {
            "type": "Point",
            "coordinates": [
                14.51,
                46.05
            ]
        }
    }
}
~~~

A mapping reads this placemark's name as `{{ stations.properties.name }}`. A placemark outside any folder has `id`, `properties`, and `geometry` at the top level of the record, with no `folder` member, and its name is `{{ properties.name }}`.

Cassiopeia places the placemark's `name`, `description`, time (see below), and extended data in `properties`, along with the name of its folder as `folder`. Its `id` attribute becomes `id`, and supported KML geometry becomes GeoJSON under `geometry`. A KML `LinearRing` becomes a GeoJSON `LineString`, matching the source semantics of a standalone ring.

A KML `MultiGeometry` may mix geometry kinds, which GeoJSON's multi-geometries cannot. Cassiopeia folds one into the multi-geometry of its members' family: a single member becomes that geometry, points become a `MultiPoint`, line strings and linear rings a `MultiLineString`, polygons a `MultiPolygon`, and a nested `MultiGeometry` contributes its own members to the same fold. A `MultiGeometry` mixing families has no GeoJSON equivalent and produces no `geometry` key, exactly like a KML geometry kind GeoJSON does not describe.

### Placemark times

KML dates a placemark with a time primitive, and Cassiopeia carries it into `properties` so a mapping can use it. A `<TimeStamp>` becomes `timeStamp`, holding the value of its `<when>`. A `<TimeSpan>` becomes a `timeSpan` object holding whichever of `begin` and `end` the span declares, so an open-ended span has only one of them. A dated KML export can therefore feed a temporal mapping directly, with `{{ properties.timeStamp }}` as the source of `observedAt`, or `{{ properties.timeSpan.begin }}` and `{{ properties.timeSpan.end }}` as the bounds of an observation window. For a placemark inside a folder, the same values sit under the folder's namespace, such as `{{ stations.properties.timeStamp }}`.

Cassiopeia copies the values as written, trimming only surrounding whitespace, and keeps them as strings. An empty `<when>`, or a span without a non-empty bound, adds nothing. Extended data with the same name as `timeStamp` or `timeSpan` takes precedence, as it does over `name` and `description`. Only a placemark's own time primitive is read. One declared on an enclosing `Folder` or `Document` is not applied to the placemarks inside it, and the `<when>` elements of a `gx:Track` are not read.

KML allows a full date and time with or without a UTC offset, a date, a year and month, or a year alone. Declare `transformation: "datetime"` on the attribute or `observedAt` that reads one. That transformation reads a date and time with a UTC offset or `Z`, such as `2026-08-03T10:00:00Z`, and reads a date alone, such as `2026-08-03`, as midnight UTC. It does not read a year alone, a year and month, or a `T`-separated date and time without an offset, such as `2026-08-03T10:00:00`. An attribute with such a value is dropped, and an `observedAt` with one is left off its attribute, and the run warns in both cases. When the source writes these shapes, map the attribute without a transformation to keep the value as a string.

### Folders and collections

A placemark inside a folder carries that folder's name as its collection label. Cassiopeia preserves the original folder name, including spaces and capitalization, so a manifest can route different folders to different mappings. It also nests folder data under a snake-case namespace in the record. For example, a folder named `Camera Area` is available through `camera_area`, as in `{{ camera_area.properties.name }}`. In nested folders, the innermost named folder supplies both the label and the namespace.

A placemark outside a folder has no collection label. A mapping binding that expects a collection cannot match it.

## XML

Generic XML is deliberately schema-less. Cassiopeia folds the XML tree into JSON-like values and uses one deterministic rule to split records. It does not guess which element represents a record.

### XML value shape

An element with only text becomes a string. An empty element becomes `null`. An element with attributes, children, or processing instructions becomes an object:

~~~xml
<station code="A-17">Main station</station>
~~~

~~~json
{
    "station": {
        "@code": "A-17",
        "#text": "Main station"
    }
}
~~~

XML attributes use an `@` prefix. Text in a mixed-content object uses `#text`, and processing instructions use `#pi`. Cassiopeia keeps namespace prefixes in element and attribute names.

### Record splitting

If the root has exactly one repeated child element directly beneath it, each occurrence becomes a record. For example:

~~~xml
<data>
    <station><id>A-17</id><name>Main station</name></station>
    <station><id>A-18</id><name>North station</name></station>
</data>
~~~

This produces records with `id` and `name` fields, so a mapping can use `{{ id }}` and `{{ name }}` directly.

If the root has no repeated child, the whole root becomes one record. If it has two or more repeated child names, Cassiopeia also keeps the whole root as one record instead of choosing one repeated element arbitrarily. A repeated element under an intermediate wrapper is not used as the split point. When the source structure does not identify a record boundary unambiguously, Cassiopeia preserves the structure for the mapping to handle.

When a repeated child is a scalar rather than an object, its record contains the value under `#text`:

~~~xml
<data><value>21.5</value><value>20.9</value></data>
~~~

Each record therefore has a `#text` member containing `21.5` or `20.9`.

## ESRI Shapefile

A Shapefile source can be a local `.shp` main file with its companion files beside it, or a ZIP bundle containing one or more layers and their companions. Cassiopeia reads the feature geometry and dBase attributes, then emits one record per feature.

Each record has this general shape:

~~~json
{
    "properties": {
        "NAME": "Main station",
        "ACTIVE": true,
        "VALUE": 21.5
    },
    "geometry": {
        "type": "Point",
        "coordinates": [
            14.51,
            46.05
        ]
    }
}
~~~

dBase fields are under `properties`. Cassiopeia converts their values to JSON where possible: character and memo fields become strings, numeric fields become numbers, logical fields become booleans, and date fields become ISO-style date strings. It omits null fields. dBase tooling may uppercase field names, so inspect the actual property names when writing a mapping.

### Geometry and coordinate reference systems

Shapefile geometries are emitted as GeoJSON for use as NGSI-LD `GeoProperty` values. When a `.prj` companion is present, Cassiopeia parses its coordinate reference system and reprojects the geometry to EPSG:4326, using WGS84 longitude and latitude. Without a `.prj` file, it assumes the coordinates are already EPSG:4326 and emits them unchanged.

The `.dbf` companion is required because it carries the feature attributes. A bare `.shp` without its required companion files is not a complete input.

### Multiple layers

A ZIP containing one Shapefile layer produces ordinary records without a collection label. A ZIP containing several `.shp` layers gives each layer a collection label based on its filename without the extension. Use collection-specific manifest bindings when the layers need different mappings.

## GeoPackage

A GeoPackage source is an OGC GeoPackage file (`.gpkg`): a SQLite database that lists its data tables in a `gpkg_contents` catalog. Cassiopeia reads every table listed with the `features` data type and every table listed with the `attributes` data type, then emits one record per row. Tile and gridded-coverage tables hold raster pyramids rather than records, so Cassiopeia skips them.

Each record has this general shape:

~~~json
{
    "properties": {
        "fid": 1,
        "name": "Main station",
        "active": true,
        "level": 21.5,
        "opened": "2026-06-03"
    },
    "geometry": {
        "type": "Point",
        "coordinates": [
            14.51,
            46.05
        ]
    }
}
~~~

Table columns are under `properties`, named exactly as in the table. The primary key comes first under its own column name, usually `fid`, so a mapping can use it in the entity identifier. Cassiopeia converts each value by its column's declared type: `BOOLEAN` becomes a boolean, the integer types and `FLOAT`, `DOUBLE`, and `REAL` become numbers, `TEXT` stays a string, `DATE` becomes an ISO 8601 date such as `2026-06-03`, and `DATETIME` becomes an ISO 8601 timestamp such as `2026-06-03T12:30:05.250Z`. Besides the strict GeoPackage form, `DATETIME` text in the common ISO 8601 variants is accepted, such as second precision, a space separator, or a numeric UTC offset; an offset is kept in the output. A `BLOB` column becomes its standard base64 encoding, the form GDAL uses for binary fields when it writes GeoJSON. Cassiopeia omits `NULL` columns.

Rows of an attributes table have no `geometry` member. A feature whose geometry is `NULL` or empty has no `geometry` member either.

### Geometry and coordinate reference systems

GeoPackage geometries are emitted as GeoJSON for use as NGSI-LD `GeoProperty` values. Each feature table names its coordinate reference system through the `srs_id` of its geometry column, which `gpkg_spatial_ref_sys` defines. Cassiopeia reads that definition, preferring the WKT2 form of the `gpkg_crs_wkt` extension when the file has one, and reprojects the geometry to EPSG:4326, using WGS84 longitude and latitude. An EPSG:4326 table is emitted unchanged. A table whose definition is `undefined`, as it is for the reserved `srs_id` values `0` and `-1`, is assumed to be EPSG:4326 already: Cassiopeia emits its coordinates unchanged and logs a warning. A geometry column whose `srs_id` has no `gpkg_spatial_ref_sys` row fails the read.

Geometries keep their X and Y coordinates only; Z and M values are dropped. Curved geometry types, such as `CIRCULARSTRING` and `COMPOUNDCURVE`, have no GeoJSON form, and a row holding one fails the read.

### Build feature

GeoPackage support is the `geopackage` Cargo feature, on in the default build. It compiles a bundled SQLite from C and links it statically. A build without it, such as one made with `--no-default-features`, still detects a GeoPackage but reports it as unsupported instead of reading it. Run `cassiopeia --version` to see whether a binary has it: its `features` line shows `+geopackage` or `-geopackage`. See [GeoPackage and SQLite](getting-started.md#geopackage-and-sqlite).

### Multiple tables

A GeoPackage with one record table produces ordinary records without a collection label. A GeoPackage with several record tables, counting both feature and attributes tables, gives each table's records a collection label equal to the table name. Use collection-specific manifest bindings when the tables need different mappings.

## GRIB

GRIB is a binary format for gridded meteorological and climate data. Cassiopeia supports GRIB1 and GRIB2 through the single `grib` source format and reads the edition from the file header.

GRIB stores one field per parameter, while an NGSI-LD observation often needs several parameters at one location. Cassiopeia groups fields that share a grid, vertical level, reference time, and forecast time, then pivots them into one record for each unmasked grid cell. The resulting record contains:

- `latitude` and `longitude` as separate numeric fields.
- `level_type` and `level` when the field declares a vertical level (see [Accessing the level](#accessing-the-level)).
- `referenceTime` when present.
- `forecastTime` when present.
- One field for each parameter, keyed by its Cassiopeia-canonical name.

For example, a record may look like:

~~~json
{
    "latitude": 46.0,
    "longitude": 14.0,
    "level_type": "height_above_ground",
    "level": 2,
    "referenceTime": "2026-08-23T00:00:00+00:00",
    "forecastTime": "2026-08-23T00:00:00+00:00",
    "temperature": 285.69,
    "dewpoint": 284.99,
    "humidity": 95.3
}
~~~

Masked parameter values are omitted from a record. A grid cell for which every parameter is masked produces no record.

### Canonical parameter names

Both editions use the same name for a physical quantity, regardless of backend: a short, space-free, level-independent `snake_case` key. The key is meant to be read directly in a template, as in `{{ temperature }}`. A spaced WMO name would require `this['...']` bracket access. A kebab-case name such as `wind-u` would need it too: Cassiopeia rejects `{{ wind-u }}` when it loads the mapping, because Tera would read it as the subtraction `wind - u`, and the error suggests `this['wind-u']` instead. The same mapping therefore works across editions and backends. The canonical set is a curated vocabulary. Here are some of the most common keys:

| Canonical key | Meaning | GRIB2 (discipline, category, number) | GRIB1 indicator |
| --- | --- | --- | --- |
| `temperature` | Air temperature | 0, 0, 0 | 11 |
| `dewpoint` | Dew point temperature | 0, 0, 6 | 17 |
| `humidity` | Relative humidity | 0, 1, 1 | 52 |
| `precip` | Total precipitation | 0, 1, 8 | 61 |
| `snow_depth` | Snow depth | 0, 1, 11 | 66 |
| `wind_dir` | Wind direction | 0, 2, 0 | 31 |
| `wind_speed` | Wind speed | 0, 2, 1 | 32 |
| `wind_u` | Eastward wind component | 0, 2, 2 | 33 |
| `wind_v` | Northward wind component | 0, 2, 3 | 34 |
| `gust` | Wind gust speed | 0, 2, 22 | 180 |
| `pressure` | Pressure | 0, 3, 0 | 1 |
| `pressure_msl` | Pressure reduced to MSL | 0, 3, 1 | 2 |
| `geopotential_height` | Geopotential height | 0, 3, 5 | 7 |
| `cloud_cover` | Total cloud cover | 0, 6, 1 | 71 |
| `cape` | Convective available potential energy | 0, 7, 6 | 157 |
| `visibility` | Visibility | 0, 19, 0 | 20 |

A parameter outside this vocabulary falls back to a stable synthetic code key: `d{discipline}c{category}n{number}` for GRIB2, identical from either backend, such as `d0c99n5`; and `g1t{table}c{centre}p{indicator}` for a GRIB1 local parameter. GRIB itself stores only numeric codes, not author-set names, so the fallback is a code. GRIB2 identifies a parameter by its `(discipline, parameterCategory, parameterNumber)` triple, while GRIB1 identifies it by `indicatorOfParameter`. The human-readable name comes from lookup tables, including the WMO master tables and centre-local tables, not from the message. A parameter missing from those tables genuinely has no name, so the synthetic code is the accurate representation. The curated vocabulary is large enough that this fallback is rare.

### Accessing the level

After normalization, a record carries two level fields that can be used directly in templates: `level_type`, a `snake_case` token, and `level`, the numeric value in that type's canonical unit. Isobaric levels use hectopascals, while heights and depths use metres. Surface-like types such as `surface` and `mean_sea_level` have no level value. Neither field needs `this[...]`:

~~~json5
pressureLevel: {
    source: "{% if level_type == 'isobaric' %}{{ level }}{% endif %}",
    transformation: "integer",
}
~~~

The two coordinate fields can become a GeoJSON point with this mapping:

~~~json5
location: {
    type: "GeoProperty",
    source: [
        "{{ longitude }}",
        "{{ latitude }}",
    ],
    transformation: "point",
}
temperature: {
    source: "{{ temperature }}",
    transformation: "float",
}
~~~

A gridded product often stores wind as eastward and northward components rather than as speed and direction. When the two components share a grid, level, and time, Cassiopeia pivots them into columns in one record. The `wind_speed` and `wind_direction` template helpers can then derive the reported quantities directly:

~~~json5
windSpeed: {
    source: "{{ wind_speed(u=wind_u, v=wind_v) }}",
    transformation: "float",
}
windDirection: {
    source: "{{ wind_direction(u=wind_u, v=wind_v) }}",
    transformation: "integer",
}
~~~

Parameters measured at different vertical levels go into separate records because the level helps identify a grid slice. Two-metre temperature and ten-metre wind are therefore distinct records. Mapping them to the same entity identity, such as a code derived from the shared cell coordinates, merges their attributes into one observation. The [GRIB1 example](https://github.com/vela-tools/cassiopeia-examples/blob/main/examples/18-grib1-derived-values/example.md) works through this process from beginning to end.

### Backends and features

Two compile-time features select the backend for each edition. Both link the ecCodes C library, which the build compiles from source, can be toggled independently, and are enabled in the default build:

- `grib2-full` routes GRIB2 through ecCodes. ecCodes computes coordinates for projected grids and reads the full parameter tables. With this feature off, GRIB2 falls back to the pure-Rust grib-rs decoder, which has no C dependency but handles only regular latitude/longitude grids.
- `grib1` routes GRIB1 through ecCodes. With this feature off, the build reports GRIB1 as unsupported because there is no pure-Rust GRIB1 decoder.

Both backends use the same canonical vocabulary and level normalization, so changing the backend does not require a different mapping. A build made with `--no-default-features` supports only regular-grid GRIB2 through grib-rs and rejects GRIB1. Add `--features grib1` or `--features grib2-full` to re-enable either ecCodes path.

With the default build, ecCodes can decode projected GRIB2 grids, such as the Lambert conformal or polar stereographic grids used by regional models like HRRR. It handles these alongside global regular-grid products such as GFS and the ICON global model. Only the pure-Rust fallback, built with `grib2-full` off, is limited to regular latitude/longitude grids. With that fallback, a projected file fails to ingest instead of producing records. The two worked examples cover both editions: [GRIB1 regional](https://github.com/vela-tools/cassiopeia-examples/blob/main/examples/18-grib1-derived-values/example.md) and [GRIB2 global](https://github.com/vela-tools/cassiopeia-examples/blob/main/examples/19-grib2-byte-range/example.md).

## NetCDF

NetCDF is a self-describing format for gridded and station data. Copernicus services (CAMS air quality, CMEMS ocean, C3S and ERA5), national meteorological services, and ERDDAP and NCEI archives publish it. Cassiopeia reads the classic CDF-1, CDF-2, and CDF-5 formats and NetCDF-4, which is stored in HDF5, through the single `netcdf` source format. The reader is pure Rust, so every build includes it, the release binaries too, and it needs no system library.

A NetCDF file produces the same wide location records as GRIB, so the same mapping style builds `AirQualityObserved`, `WeatherObserved`, or `SeaConditions` entities from either format. A file without a global `featureType` attribute is read as a grid, and so is one that declares `featureType = "GRID"`, which THREDDS servers write on every grid their NetCDF Subset Service returns. A file that declares `featureType = "timeSeries"` is read as station time series, as described in [Station time series](#station-time-series). Any other feature type is rejected.

### Gridded records

Each data variable located by latitude and longitude produces one record per unmasked cell of every latitude-longitude plane. Data variables with the same ordered dimensions and the same listed scalar and auxiliary coordinates pivot into columns of the same record, so a 2 m temperature and a 10 m wind told apart only by their scalar `height` produce records of their own. A record contains:

- `latitude` and `longitude` as separate numeric fields. A longitude in `[0, 360)` is folded into `[-180, 180)`.
- `time` when the variable has a time dimension, as an RFC 3339 timestamp (see [Time](#time)).
- `level_type` and `level` when the variable has a vertical dimension that maps onto a level (see [Levels](#levels)).
- One field for every other dimension, keyed by the name of its coordinate variable, such as an ensemble member `number` or a `forecast_reference_time`.
- One field for each scalar coordinate, and each auxiliary coordinate over those other dimensions, that the data variable lists in its `coordinates` attribute. CF 1.11 section 5.7 makes a scalar coordinate equivalent to a dimension of length one, so a scalar `time` becomes the record's `time` and a scalar `height` its level. Any other is keyed by its variable name, such as the `reftime` or `time_run` forecast reference time THREDDS writes along the time dimension. When a dimension already gives the record its time or level, a second time or vertical coordinate is keyed by its variable name too.
- One field for each data variable, keyed by its NetCDF variable name.

For example, a record from a pressure-level file with temperature `t` and relative humidity `r` may look like:

~~~json
{
    "latitude": 46.0,
    "longitude": 14.5,
    "time": "2026-08-23T06:00:00+00:00",
    "level_type": "isobaric",
    "level": 850,
    "t": 285.3,
    "r": 71.2
}
~~~

Values are unpacked with `scale_factor` and `add_offset`. A value equal to `_FillValue` or `missing_value`, or outside `valid_min`, `valid_max`, or `valid_range`, counts as masked and is omitted from the record. A variable without a `_FillValue` attribute is also masked at its type's netCDF default fill value, the value the library leaves in an element no writer set; byte variables are exempt, since their whole range is commonly real data. A cell masked in every data variable produces no record. Data variable names are kept exactly as the producer wrote them. A name that is not a plain identifier, such as one containing a hyphen, is read in a template with `this['...']`.

### Coordinate roles

Cassiopeia recognizes a coordinate by its CF attributes, never by its name. It checks the attributes CF 1.11 section 4 names in this order: `standard_name`, then `units`, then `axis`, then `positive`.

| Role | Recognized by | Record key |
| --- | --- | --- |
| Latitude | `standard_name` `latitude`, or `units` `degrees_north`, `degree_north`, `degree_N`, `degrees_N`, `degreeN`, or `degreesN` | `latitude` |
| Longitude | `standard_name` `longitude`, or `units` `degrees_east`, `degree_east`, `degree_E`, `degrees_E`, `degreeE`, or `degreesE` | `longitude` |
| Time | `standard_name` `time`, `axis` `T`, or `<unit> since <epoch>` units on a variable with no other `standard_name` | `time` |
| Vertical | a vertical `standard_name` such as `air_pressure`, `height`, `altitude`, `depth`, `model_level_number`, or a dimensionless coordinate such as `atmosphere_sigma_coordinate`; pressure `units`; `axis` `Z`; or a `positive` attribute | `level_type` and `level`, or the variable name when it maps onto no level |
| Projected | `standard_name` `projection_x_coordinate`, `projection_y_coordinate`, `grid_longitude`, or `grid_latitude`, or `axis` `X` or `Y` without degree units | none on a grid, where the variable is skipped; the variable name on a station |
| Other | anything else | the variable name |

A variable called `lat` without these attributes is therefore not a latitude. A `forecast_reference_time` with epoch units has a standard name of its own, so it is emitted as a timestamp under `forecast_reference_time` rather than competing with the valid `time`.

A data variable is a numeric variable with at least two dimensions that is neither a coordinate variable nor named by another variable's `bounds`, `climatology`, or `grid_mapping` attribute. It is skipped, with a warning that names it and the reason, when it holds characters or strings, when a dimension has no coordinate variable, when its horizontal axes are projected or curvilinear, or when more than one dimension has the time role or the vertical role. The run fails when no data variable remains. It also fails when two values would land under one record key, such as a data variable named `time` on a grid that also has a time coordinate, rather than letting one overwrite the other.

### Levels

A vertical coordinate maps onto the same `level_type` tokens GRIB uses, so a level condition in a template works for both formats:

| CF vertical coordinate | `level_type` | `level` |
| --- | --- | --- |
| Pressure in `Pa`, `hPa`, `mbar`, or `millibar` | `isobaric` | hPa |
| `standard_name` `height` in metres | `height_above_ground` | metres |
| `standard_name` `altitude` or `height_above_mean_sea_level` in metres | `altitude_above_mean_sea` | metres |
| `standard_name` `depth`, or `positive = "down"` and no vertical `standard_name`, in metres, under soil quantities | `depth_below_land` | metres |
| The same depth under sea water quantities | `depth_below_sea` | metres |
| `atmosphere_sigma_coordinate` | `sigma` | the coordinate value |

A vertical `standard_name` other than `air_pressure` decides on its own, ahead of the units and of `positive`. A hybrid coordinate that declares `hPa`, as CAM history files do for their nominal `1000 * (a + b)` values, is therefore not read as a pressure. Any other vertical coordinate is emitted under its own variable name with its raw value. That includes a `model_level_number`, a height in kilometres, a `depth_below_geoid`, an ocean s-coordinate, and the hybrid sigma-pressure and hybrid height coordinates, whose values are not the model level numbers GRIB's `hybrid` level carries.

CF's `depth` is the distance below a surface it does not name, so the data variables on the coordinate name it: those that span its dimension or list it in their `coordinates` attribute. A `standard_name` with the word `soil`, such as `soil_temperature` or `mass_content_of_water_in_soil_layer`, is a soil quantity, measured below the land surface like GRIB's `depth_below_land`. One with `sea_water` or `ocean`, such as `sea_water_salinity` or `ocean_mixed_layer_thickness`, is a sea water quantity. A variable without a standard name, such as a quality flag, or with one that is neither, such as `sea_ice_temperature`, does not count. A depth that no variable gives a surface, or that soil and sea water quantities share, is emitted under its own variable name with its raw value.

### Time

Every coordinate whose units take the form `<unit> since <epoch>` is decoded into an RFC 3339 timestamp ending in `+00:00`, whatever its role. The unit can be any UDUNITS spelling of seconds, minutes, hours, days, or weeks, such as `sec`, `msec`, or `milliseconds`, or `months` in the `360_day` calendar. Times are rounded to the microsecond, the finest a double counting days or seconds from a distant epoch can resolve, so a ten-minute series stored as fractional days reads `04:10:00` rather than `04:09:59.999999744`. A fraction of a second is written with its trailing zeros trimmed. The supported calendars are `standard` (also `gregorian`), `proleptic_gregorian`, `noleap` (also `365_day`), `all_leap` (also `366_day`), `360_day`, and `julian`. A variable without a `calendar` attribute uses `standard`. A date in a non-Gregorian calendar is written as that calendar states it, so day 59 of a `360_day` year is `2026-02-30T00:00:00+00:00`.

The epoch may end with a UTC designator: `Z`, `UTC`, `GMT`, or a zero offset such as `+00:00`, `+0000`, or the unsigned `0:00` the NCEI netCDF templates prescribe. The run fails when the epoch names a non-zero offset, when the `calendar` attribute names anything else (including `utc`, `tai`, and `none`, whose leap-second or calendar-free arithmetic Cassiopeia does not perform), or when the units contain non-ASCII characters. A coordinate in the time role whose units name no epoch, such as the `day as %Y%m%d.%f` CDO writes for an absolute time axis, is emitted as its raw number.

### Station time series

A file that declares `featureType = "timeSeries"` holds observations at fixed stations, a CF discrete sampling geometry (CF 1.11 section 9 and Appendix H.2). Cassiopeia emits one record per observation and recognizes all five CF layouts:

| Layout | How it is recognized |
| --- | --- |
| Single | The latitude is a scalar, and the data runs along the time dimension. |
| Orthogonal multidimensional | The data has a station and a time dimension, in either order, and all stations share the `time(time)` coordinate. |
| Incomplete multidimensional | The time is `time(station, obs)`, padded with fill values where a station has fewer observations. |
| Contiguous ragged | A count variable carries `sample_dimension`; each station's observations follow those of the station before it. |
| Indexed ragged | An index variable carries `instance_dimension`; each observation names its station. |

A dimension of length one that is neither the station nor the time dimension holds one value for every observation. An OceanSITES station file, whose `LATITUDE`, `LONGITUDE`, and `HEIGHT` each have a dimension of length one, is therefore read as a single station, with `HEIGHT` as its level. A file whose latitude runs along a station dimension and whose time runs along another is read as orthogonal only when some variable spans both dimensions. Otherwise it is a ragged file whose count or index variable lacks its `sample_dimension` or `instance_dimension`, and the run fails rather than repeating every observation at every station.

The coordinates come from the variables the data variables list in their `coordinates` attribute, the coordinate variables, and the variable whose `cf_role` is `timeseries_id`. The latitude, longitude, and time are required, and the run fails when one is missing. A vertical station coordinate, such as an `altitude`, becomes `level_type` and `level`. A station name or identifier is emitted as a string under its own name, and any other listed coordinate as a number or a string under its own name. A station variable that no `coordinates` attribute lists and that has no `cf_role` is not emitted. A text variable that varies by observation, such as the per-observation `station_name` ERDDAP writes or an OceanSITES data mode flag, is emitted as a string under its own name. An empty string counts as missing. For example:

~~~json
{
    "latitude": 46.07,
    "longitude": 14.52,
    "time": "2026-08-23T06:00:00+00:00",
    "level_type": "altitude_above_mean_sea",
    "level": 299,
    "station_name": "LJUBLJANA",
    "pm10": 18.2,
    "no2": 21.5
}
~~~

An observation whose time is masked is padding and is skipped. A masked data value is omitted from its record, and an observation masked in every data variable produces no record. A count or index that points outside its dimension fails the run. When the counts of a contiguous ragged array cover fewer elements than its sample dimension, the elements left over belong to no station and are skipped with a warning.

### Limitations

- Only the root group is read. A NetCDF-4 file with subgroups logs a warning and ignores them.
- Projected and curvilinear grids are skipped, because their cells have no latitude and longitude coordinate variables to read. That includes a Lambert grid whose two-dimensional latitude and longitude are auxiliary coordinates, as the HRRR and MEPS forecasts are. Feature types other than `timeSeries`, such as `point`, `trajectory`, `profile`, `timeSeriesProfile`, and `trajectoryProfile`, are rejected.
- A station file's coordinates and data variables are read whole and held in memory together, along with one entry per observation, so a very large station archive needs memory for all of its variables at once.
- NetCDF-4 variables compressed with deflate, shuffle, Fletcher-32, SZIP, N-bit, scale-offset, or LZ4 are read. A variable compressed with another HDF5 filter, such as zstd or Blosc, or stored as an HDF5 virtual dataset fails to read.

## Choosing a format for a mapping

Start by looking at one profiled record, not at the original file alone. Then build the mapping around the fields Cassiopeia actually exposes:

1. Identify the record boundary: row, JSON object, GeoJSON feature, placemark, XML repeated child, Shapefile feature, GeoPackage table row, grid cell, or station observation.
2. Locate the values in the record shape, including containers such as `properties`, `geometry`, and XML `@` or `#` fields.
3. Decide whether the source value already has the required JSON type or needs a transformation.
4. For geometry, select `GeoProperty`. A complete GeoJSON geometry needs no transformation; coordinates need the geometry transformation that matches the type to build.
5. For KML folders, multi-layer Shapefiles, or multi-table GeoPackages, decide whether collection-specific mappings are needed.

The [mapping guide](mapping.md) explains the mapping language in detail. The format page tells you what the source record looks like; it does not replace the mapping's responsibility to choose the target entity identity and attribute model.

## Next steps

- [Write a mapping](mapping.md): build the entity from the record fields shown here.
- [Templates](templates.md): learn the expression language that a mapping's `source` uses.
- [Running Cassiopeia](running.md#choose-the-source): declare an input and its format on the command line.
