# Templates

A template reads values from a record. Wherever a mapping accepts a `source`, that source is a template: `{{ field }}` pulls a value, while any surrounding text stays literal. Cassiopeia uses the [Tera](https://keats.github.io/tera/) template language and adds its own filters and functions. This page covers the Tera syntax used by mappings and the complete Cassiopeia vocabulary. For the rest of Tera, follow the links to its documentation.

## Read the record

A record's fields are available by name. Use dot access to follow a path into nested objects:

~~~text
{{ name }}
{{ properties.name }}
~~~

For a field name that is not a valid identifier, such as one with spaces, parentheses, or punctuation, index it through `this`, which is the current record:

~~~text
{{ this['CO(GT)'] }}
~~~

`{{ context }}` resolves to the whole current record as a value. Use it when a transformation needs the entire source object, such as when passing a structured geometry straight through.

A reference to a field that the record does not have resolves to nothing. The attribute is dropped unless output settings keep nulls.

## Control flow

Templates can branch and loop. Conditionals choose text by a test on the record:

~~~text
{% if status == 'active' %}available{% elif status == 'idle' %}waiting{% else %}offline{% endif %}
~~~

Loops walk a list value:

~~~text
{% for code in codes %}{{ code }} {% endfor %}
~~~

The usual operators are available inside `{{ }}`: comparison (`==`, `!=`, `<`, `<=`, `>`, `>=`), logic (`and`, `or`, `not`), and arithmetic (`+`, `-`, `*`, `/`, `%`).

## Built-in filters

A filter transforms a value inside a template and follows a pipe: `{{ value | filter }}`. Tera provides many filters; these are the ones mappings use most often:

| Filter | Effect |
| --- | --- |
| `default(value=...)` | Substitute a fallback when the value is missing. |
| `upper`, `lower` | Change case. |
| `trim` | Remove surrounding whitespace. |
| `replace(from=..., to=...)` | Replace every occurrence of a substring. |
| `length` | The length of a string or list. |
| `truncate(length=...)` | Shorten a string to a maximum length. |

For string, number, date, and collection filters not listed here, see the [Tera filter documentation](https://keats.github.io/tera/docs/#built-in-filters).

## Cassiopeia filters

Cassiopeia adds these filters to the built-in set.

| Filter | Effect |
| --- | --- |
| `clean` | Fold every run of whitespace to a single space and trim the ends. `{{ name | clean }}`. |
| `get(key=..., default=...)` | Read `key` from a map value. It returns the entry, `default` if given, or nothing when the key is absent. Unlike Tera's built-in `get`, a missing key does not cause an error. |
| `json_decode` | Parse a JSON string into a structured value the template can index and iterate, the decode counterpart of `json_encode`. `{{ cast \| json_decode \| first \| get(key='id') }}`. A non-string passes through unchanged; an empty value yields nothing; only a non-empty, malformed string is an error. |
| `date_subtract_seconds(seconds=..., format=...)` | Shift a timestamp backwards by `seconds` and format it. `seconds` defaults to 0, and `format` defaults to `%Y-%m-%dT%H:%M:%SZ`. The input may be an epoch number or a textual date-time. |

### Math filters

Cassiopeia adds mathematical filters for deriving a target quantity from the numbers a source stores. Each filter takes the one piped value, which may be a number or the numeric string carried by a text source:

| Filter | Result |
| --- | --- |
| `sqrt`, `cbrt` | Square root, cube root. |
| `sign` | `-1`, `0`, or `1` by the value's sign. |
| `exp`, `ln`, `log10`, `log2` | The exponential and the natural, base-10, and base-2 logarithms. |
| `floor`, `ceil`, `trunc` | Round toward negative infinity, toward positive infinity, and toward zero. |
| `sin`, `cos`, `tan` | Circular functions of an angle in radians. |
| `asin`, `acos`, `atan` | Inverse circular functions, returning radians. |
| `radians`, `degrees` | Convert degrees to radians and back. |

## Cassiopeia functions

A function takes named arguments: `{{ function(arg=..., ...) }}`. Every numeric argument may be a number or a numeric string.

| Function | Result |
| --- | --- |
| `dms_point(value=...)` | Parse a labelled degrees-minutes-seconds coordinate pair into a GeoJSON `Point` in `[longitude, latitude]` order. For example, `dms_point(value="27°59′17″N 86°55′30″E")`. |
| `geohash(lat=..., lon=..., precision=...)` | Encode a latitude and longitude into a geohash string. `precision` defaults to 9. |

### Geometry functions

These reach the same conversion lattice as a GeoProperty's [`geometry` block](mapping.md#convert-between-geometry-types) for a geometry that must be produced inside a structure a `transformation` cannot reach. Each takes the source geometry as `value`, either as a GeoJSON geometry object or as its JSON text. If the source cannot satisfy the conversion, the function yields nothing and the surrounding attribute is dropped. A misconfigured call, such as an unknown type or conversion or a `value` that is not a geometry, fails the record.

| Function | Result |
| --- | --- |
| `geo_convert(value=..., to=..., using=...)` | Convert a geometry to the type `to` names, using the same tokens a `transformation` does. `using` names a conversion from the same vocabulary as the `geometry.convert` field and may be omitted for lossless conversions. For example, `geo_convert(value=geometry, to="point", using="point-on-surface")`. |
| `geo_centroid(value=...)` | The geometry's centroid, as a `Point`. |
| `geo_bbox(value=...)` | The geometry's bounding box, as a rectangular `Polygon`. |
| `geo_area(value=...)` | The area the geometry encloses, in square metres, measured geodesically. A geometry below dimension two measures zero. |
| `geo_length(value=...)` | The length of the geometry, in metres, measured geodesically. A curve measures its own length, a surface its perimeter. |

### Math functions

| Function | Result |
| --- | --- |
| `hypot(x=..., y=...)` | The vector magnitude, `sqrt(x^2 + y^2)`. |
| `clamp(value=..., min=..., max=...)` | `value` constrained to the closed interval `[min, max]`. |
| `map_range(value=..., in_min=..., in_max=..., out_min=..., out_max=...)` | `value` rescaled linearly from one span onto another. |
| `atan2(y=..., x=...)` | The angle in radians of the point `(x, y)`, using both signs for the quadrant. |
| `pi()`, `tau()`, `e()` | The constants pi, tau (two pi), and Euler's number. |
| `bearing(east=..., north=..., convention=...)` | A compass bearing in degrees clockwise from north for an east/north vector. |
| `wind_speed(u=..., v=...)` | Wind speed from its eastward and northward components. This is an alias of `hypot`. |
| `wind_direction(u=..., v=...)` | Meteorological wind direction from the same components. This is an alias of `bearing` with its `from` convention. |

The `bearing` `convention` argument chooses which direction the bearing names: `"from"` (the default, the meteorological convention, where the vector comes from) or `"to"` (where it points). A vector pointing due east reads `90` under `"to"` and `270` under `"from"`. This is the difference between reporting wind by the direction it blows from and a current by the direction it flows to.

## Additional filters, functions, and tests

Cassiopeia also enables the following parts of Tera's contributed set.

Filters: `b64_encode`, `b64_decode`, `date`, `filesize_format`, `format`, `json_encode`, `regex_replace`, `shuffle`, `slug`, `spaceless`, `striptags`, `urlencode`, `urlencode_strict`.

Functions: `get_random`, `now`.

Tests, used in a condition as `{% if value is <test> %}`: `after`, `before`, `matching`.

The [Tera documentation](https://keats.github.io/tera/docs/) describes each of these in detail.

## When a computation has no answer

A math filter or function whose result is not a finite number resolves to nothing, and Cassiopeia omits the attribute just as it would omit a missing field. The rest of the entity is unaffected. This covers the square root of a negative, the logarithm of zero, an inverse sine outside `[-1, 1]`, a `map_range` over a zero-width input span, and any other computation that produces `NaN` or infinity. A domain-invalid computation never fails the whole record.

This is distinct from a misconfigured input. A math helper given a value that is not a number at all, such as a word where a number was expected, returns an error and fails the record rather than producing null. An out-of-domain number drops one attribute. A field that was never numeric is a mapping mistake worth stopping for.

## Next steps

- [Choosing a data model](data-models.md): choose the entity type and `@context` your mapping targets.
- [Write a mapping](mapping.md#more-filters-and-functions): see where these expressions are used.
- [Tera documentation](https://keats.github.io/tera/docs/): read the base template language documentation.
