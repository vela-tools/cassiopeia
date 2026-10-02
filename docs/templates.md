# Templates

A template reads values from a record. Wherever a mapping accepts a `source`, that source is a template: `{{ field }}` pulls a value, while any surrounding text stays literal. Cassiopeia uses the [Tera](https://keats.github.io/tera/) template language and adds its own filters and functions. This page covers the Tera syntax used by mappings and the complete Cassiopeia vocabulary. For the rest of Tera, follow the links to its documentation.

## Read the record

A record's fields are available by name. Use dot access to follow a path into nested objects:

~~~text
{{ name }}
{{ properties.name }}
~~~

A `{{ }}` that holds nothing but a field reference is a plain reference, and Cassiopeia reads it straight from the record without Tera. Whitespace inside the braces does not matter. A plain reference is one of these:

- A name, or a dotted path of names such as `properties.name`. A name starts with a letter or `_` and continues with letters, digits, and `_`. Letters outside ASCII count, so `{{ čas }}` and `{{ ulica.številka }}` are plain references.
- `this['key']`, a single-quoted key that contains no `.` and no `\`.
- `this[n]`, a column number, for a source without a header row.

Tera reads `true`, `false`, `none`, `null`, `not`, and a bare `this` as keywords, so `{{ none }}` is not a reference to a field called `none`. Read such a field as `{{ this['none'] }}`.

A template made only of literal text and plain references, such as `Station-{{ id }}` or `{{ first }} {{ last }}`, joins them as text. Tera evaluates every other template: one with a filter, an operator, a literal, a tag, a comment, or whitespace control.

For a field name that is not a valid identifier, such as one with spaces, parentheses, hyphens, or punctuation, index it through `this`, which is the current record:

~~~text
{{ this['CO(GT)'] }}
~~~

Inside a filter, an operator, or a tag, Tera reads only ASCII names. Write a field with a non-ASCII name through `this` there as well, as in `{{ this['čas'] | upper }}`, or `{{ ulica['številka'] | upper }}` for a nested one. Cassiopeia rejects `{{ čas | upper }}` when it loads the mapping.

`{{ context }}`, written as the whole `source` string and spaced any way, resolves to the whole current record as a value. Use it when a transformation needs the entire source object, such as when passing a structured geometry straight through.

A plain reference to a field the record does not have, or holds as null, resolves to nothing, and Cassiopeia omits the attribute. A field that holds empty text resolves to empty text, which the default `skipNull` output setting also omits (see [Handle null and empty values](output.md#handle-null-and-empty-values)). A template of literal text and plain references resolves to nothing as soon as any of its fields is missing, null, or empty, so `{{ first }} {{ last }}` over a record with no `last` omits the attribute rather than writing `John null` or `John `.

Inside a template that Tera evaluates, the operation decides what a missing or null field does. A filter such as `upper` or `split` fails on a missing or null field. Arithmetic, an ordering comparison such as `>`, and printing a field the record does not have fail on a missing field. Other operations treat the field as empty: `~` joins it as empty text, `==` compares it as unequal, and printing a null field prints nothing. Guard or default a field that can be absent, as in `{% if code %}{{ code | split(pat=' ') }}{% endif %}`.

A template that fails to render costs only the attribute it belongs to. [When a template fails](#when-a-template-fails) covers what Cassiopeia reports and how to fix it.

## Control flow

Templates can branch and loop. Conditionals choose text by a test on the record:

~~~text
{% if status == 'active' %}available{% elif status == 'idle' %}waiting{% else %}offline{% endif %}
~~~

Loops walk a list value. Every iteration is rendered, so a list holding `a` and `b` gives `a b `:

~~~text
{% for code in codes %}{{ code }} {% endfor %}
~~~

The usual operators work inside `{{ }}` without a filter: comparison (`==`, `!=`, `<`, `<=`, `>`, `>=`), logic (`and`, `or`, `not`), arithmetic (`+`, `-`, `*`, `/`, `%`), and `~` to join text, as in `{{ n + 1 }}` or `{{ code ~ '-' ~ id }}`.

Put a space on at least one side of a `-` between two names. `{{ a - b }}` subtracts, while `{{ a-b }}` is rejected because it reads like a field called `a-b`. Next to a number, `{{ n-1 }}` subtracts as written. To read a field whose name has a hyphen, use `{{ this['station-id'] }}`.

`{# … #}` is a comment and renders nothing. A `-` just inside a delimiter, as in `{{- name -}}` or `{%- if code -%}`, trims the whitespace on that side. To keep `{{`, `{%`, or `{#` as literal text, wrap it in a raw block: `{% raw %}{{ id }}{% endraw %}` renders `{{ id }}`.

## Keep a value's type

When a template's whole output is one `{{ }}` expression, the attribute receives that expression's value with its type intact:

- `split` gives an array, and `json_decode` gives whatever the text encodes.
- Arithmetic such as `{{ n + 1 }}` gives a number, and so do `round`, `int`, `float`, `length`, the math filters, and the math functions.
- A comparison such as `{{ n > 1 }}`, or a test such as `{{ code is string }}`, gives a boolean.
- `get` gives the entry with its own type.
- `geo_convert`, `geo_centroid`, and `geo_bbox` give a GeoJSON geometry object.
- `dms_point` and `geohash` give text.

This is how a text source such as CSV builds a list value from one cell:

~~~text
{{ this[2] | split(pat=' ') }}
~~~

With `transformation: "array"`, a cell holding `BS IN` becomes `["BS", "IN"]`. A cell holding the JSON text `["BS","IN"]` gives the same result through `{{ this[2] | json_decode }}`.

The attribute's `transformation` still applies to the value. When none is named, the attribute's type picks the default. On a `Property`, and on every type except `GeoProperty`, `ListProperty`, and `JsonProperty`, the default is `string`: a number becomes its text and an array or object becomes its compact JSON text, so `{{ this[2] | split(pat=' ') }}` with no transformation gives the string `["BS","IN"]`. To keep the type, name it: `integer` or `float` for a number, `boolean` for a boolean, `array` for an array, `object` for an object, or a geometry transformation such as `point` for a geometry. A `GeoProperty` defaults to `geometry`, a `ListProperty` to `array`, and a `JsonProperty` keeps the value as it is, so none of them needs a transformation to keep a geometry, a list, or a structured value. Apart from the geometry transformations, which also read a GeoJSON geometry written as JSON text, neither a transformation nor a default parses text, so decode a JSON string with `json_decode` first.

The expression may sit behind `{% if %}`, `{% elif %}`, `{% else %}`, and `{% set %}` tags and `{# #}` comments, with only whitespace around them, as long as the template holds one `{{ }}` in total. A condition that skips the expression gives no value, so the attribute is omitted:

~~~text
{% if this[2] %}{{ this[2] | split(pat=' ') }}{% endif %}
~~~

Anything else renders to text. That covers literal text around or between the tags, more than one expression, and a loop. For example, `{% if code %}{{ code }}{% else %}none{% endif %}` always gives a string. A transformation such as `integer` still reads a number out of that text.

## Built-in filters

A filter transforms a value inside a template and follows a pipe: `{{ value | filter }}`. Tera provides many filters; these are the ones mappings use most often:

| Filter | Effect |
| --- | --- |
| `default(value=...)` | Substitute a fallback when the field is missing. Add `boolean=true` to also substitute it for a null or empty value. |
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
| `clean` | Fold every run of whitespace to a single space and trim the ends. `{{ name \| clean }}`. |
| `get(key=..., default=...)` | Read `key` from a map value. It returns the entry, `default` if given, or nothing when the key is absent. Unlike Tera's built-in `get`, a missing key does not cause an error. |
| `json_decode` | Parse a JSON string into a structured value the template can index and iterate, the decode counterpart of `json_encode`. `{{ cast \| json_decode \| first \| get(key='id') }}`. As the whole expression, it gives the decoded value itself, which `transformation: "object"` or `"array"` keeps, as do a `ListProperty` and a `JsonProperty` with no transformation. A non-string passes through unchanged, and empty text yields nothing. A non-empty, malformed string fails the template, so the attribute is dropped with a warning. |
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
| `dms_point(value=...)` | Parse a labelled degrees-minutes-seconds coordinate pair into a GeoJSON `Point` in `[longitude, latitude]` order. For example, `dms_point(value="27°59′17″N 86°55′30″E")`. The result is the Point's JSON text, which a `point` transformation reads. |
| `geohash(lat=..., lon=..., precision=...)` | Encode a latitude and longitude into a geohash string. `precision` defaults to 9. |

### Geometry functions

These reach the same conversion lattice as a GeoProperty's [`geometry` block](mapping.md#convert-between-geometry-types) for a geometry that must be produced inside a structure a `transformation` cannot reach. Each takes the source geometry as `value`, either as a GeoJSON geometry object or as its JSON text. If `value` is missing, null, or empty, or the source cannot satisfy the conversion, the function yields nothing and the surrounding attribute is dropped. A misconfigured call, such as an unknown type or conversion or a `value` that is not a geometry, fails the template: the attribute is dropped with a warning (see [When a template fails](#when-a-template-fails)).

As the whole expression, `geo_convert`, `geo_centroid`, and `geo_bbox` give a GeoJSON object that a geometry transformation takes as it is, as in `{{ geo_centroid(value=geometry) }}` with `transformation: "point"`.

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

A math filter or function whose result is not a finite number resolves to nothing. When that computation is the whole expression, Cassiopeia omits the attribute without a warning, just as it would omit a missing field. The rest of the entity is unaffected. This covers the square root of a negative, the logarithm of zero, an inverse sine outside `[-1, 1]`, a `map_range` over a zero-width input span, and any other computation that produces `NaN` or infinity.

This is distinct from a misconfigured input. A math helper given a value that is not a number at all, such as a word where a number was expected, returns an error rather than producing null. The template fails, and the attribute is dropped with a warning (see [When a template fails](#when-a-template-fails)). An out-of-domain number drops one attribute silently. A field that was never numeric is a mapping mistake, so it is reported.

## When a template fails

A template can fail in two places: when Cassiopeia loads the mapping, or when it renders the template for one record.

### At load time

Cassiopeia checks every template when it loads the mapping, before it reads any input. A template that cannot work for any record stops the run. The error names the mapping file and where the template sits (`identity.entityName`, `scope`, `observedAt`, or an attribute), then says what to change:

~~~text
× The attribute `code` template in the mapping document at 'maps/thing.json5' cannot be compiled
  ╰─ `{{` opened at column 9 of `Station-{{ id` is never closed: close it with `}}`
~~~

Run with `-v` to add Tera's own message, which gives the column in your template where Tera stopped.

Cassiopeia rejects a template at load time for any of these:

- A `{{`, `{%`, or `{#`, or a `{% raw %}` block, that is never closed.
- A block such as `{% if %}` or `{% for %}` without its end tag.
- A filter, function, or test Cassiopeia does not know. When a known name is close, the message suggests it.
- Two names joined by a hyphen with no space, inside `{{ }}` or `{% %}`. Tera would read `station-id` as `station - id`, so the message offers both spellings. A hyphen in quoted text, outside the delimiters, or inside `{% raw %}` is fine.
- A non-ASCII name inside an expression or a tag. The message gives the `this['…']` spelling.
- Any other syntax Tera cannot parse.

For those cases, the second line of the error reads like this:

~~~text
`{{` opened at column 9 of `Station-{{ id` is never closed: close it with `}}`
`{% if %}` opened at column 1 of `{% if n %}{{ n + 1 }}` is never closed: add `{% endif %}`
Unknown filter `uper` in `{{ name | uper }}`: did you mean `upper`?
`station-id` in `{{ station-id }}` is ambiguous: write `this['station-id']` to read the field, or `station - id` to subtract
`čas` in `{{ čas | upper }}` is not a name Tera can read: write `this['čas']` to read the field
`{{ a b }}` is not valid template syntax at column 6
~~~

### At render time

A template can also fail for one record, typically because a field it needs is missing or null. That costs only the attribute the template belongs to. Cassiopeia omits the attribute and still writes the rest of the entity. A failure in an attribute's `properties`, nested `mappings`, or language entries drops the whole attribute, and a relationship whose `properties` fail is removed rather than written without them.

The run warns once per attribute, however many entities lost it. The warning names the template's location, the first entity that lost the attribute, and a hint:

~~~text
! The attribute `sum` template in the mapping document at 'maps/thing.json5' could not be resolved for `urn:ngsi-ld:Thing:T-b`, so attribute `sum` was dropped; the entity itself was kept
  ╰─ `{{ n + 1 }}` reads `n`, which this record does not have: guard it with `{% if n %}…{% endif %}` or default it with `n | default(value=…)`
~~~

For a field that is present but null, the hint asks for `boolean=true`, because `default` on its own replaces only a missing field:

~~~text
`{{ code | split(pat=' ') }}` reads `code`, which is null in this record: guard it with `{% if code %}…{% endif %}` or default it with `code | default(value=…, boolean=true)`
~~~

Any other failure reads `` `…` failed to render ``. Run with `-v` to see Tera's reason and how many entities lost the attribute. The run summary counts these warnings under `extractor-template-unresolvable`.

A template that names an entity fails differently. When `identity.entityName`, a `scope` template, a relationship's `source`, or a synthetic entity's identity fails for a record, Cassiopeia skips the whole record with a warning, counted under `expander-urn-ungeneratable`.

To keep a field that can be absent from failing the template, guard it or give it a default:

~~~text
{% if n %}{{ n + 1 }}{% endif %}
{{ n | default(value=0) + 1 }}
{{ code | default(value='', boolean=true) | upper }}
~~~

A guard that skips the expression gives no value, so the attribute is omitted without a warning.

## Next steps

- [Choosing a data model](data-models.md): choose the entity type and `@context` your mapping targets.
- [Write a mapping](mapping.md#more-filters-and-functions): see where these expressions are used.
- [Tera documentation](https://keats.github.io/tera/docs/): read the base template language documentation.
