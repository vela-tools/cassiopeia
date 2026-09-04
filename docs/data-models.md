# Choosing a data model

Every entity Cassiopeia writes has a data model: a `type`, the attributes that type carries, and a `@context` that gives each term a global meaning. The mapping's `dataModel` field names it. Choosing a model is one of the first decisions in a mapping. The first question is whether a published model fits at all; if it does, choose which one. This page explains the options and their trade-offs.

## What a data model is

In NGSI-LD, a data model consists of three parts:

- **An entity `type`**: the class of thing an entity is, such as `WeatherObserved` or `BicycleCounter`.
- **An attribute set**: the properties, relationships, and other attributes entities of that type carry, and the shapes those attributes take.
- **A `@context`**: the JSON-LD dictionary that maps each short term (`type`, `WeatherObserved`, `temperature`) onto a full IRI, so that two systems agree on what each term means. The [`@context` example](examples/20-csv-at-context/example.md) shows one authored from scratch.

A model is fully defined only when all three are present. A type name with no agreed attributes or context is just a label. A type name with an attribute set and a context gives two systems a shared basis for exchanging data.

## Smart Data Models are one option

[Smart Data Models](https://smartdatamodels.org/) (SDM) is a large, cross-domain catalog of published NGSI-LD models for areas such as smart cities, energy, water, and agriculture. It is governed by a community initiative. When a published model fits the data, it provides four things at once:

- **Interoperability.** An entity that conforms to a published model can be exchanged with other systems built against that model across organizations and platforms in the FIWARE and data-space ecosystem.
- **Published JSON Schemas.** Each model includes a schema that lets Cassiopeia [validate](validation.md) an entity against the model before writing it.
- **A community `@context`.** The catalog publishes a context for each model, so Cassiopeia can resolve and attach it without requiring you to author one.
- **A shared vocabulary.** Using the catalog's attribute names and enumerations means a `WeatherObserved` entity from one producer lines up field for field with one from another.

Several examples target published models: [`OffStreetParking`](examples/03-geojson-smart-data-model/example.md), [`WeatherObserved`](examples/14-xml-unit-code/example.md), and the [airport/airline graph](examples/11-csv-relationship-graph/example.md) all validate against the schemas the catalog publishes.

Smart Data Models is *an* option, not a requirement. It provides validation schemas and shared vocabularies that you can use when they fit your data. You can produce valid NGSI-LD without it.

## Custom models are first-class

Neither NGSI-LD nor Cassiopeia requires a model to come from a catalog. A `dataModel` is a name. The mapping decides which attributes it has, and you author the `@context` that gives them meaning. An entity based on a model you created is just as valid as one based on a catalog model.

The [custom-`@context` example](examples/20-csv-at-context/example.md) shows the process. The NASA Exoplanet Archive has no Smart Data Model, so the example defines an `ExoPlanet` type, writes a JSON-LD `@context` binding each term to an IRI, and attaches it with `--context local`. All three [attribute-type examples](examples/index.md), [`BicycleCounter`](examples/24-json-list-property/example.md), [`WeatherAlert`](examples/25-geojson-json-property/example.md), and [`UrbanMobilityPoint`](examples/26-csv-vocab-property/example.md), also use custom models. Each isolates one NGSI-LD attribute type, and a custom model keeps the focus on that type without incidental requirements from a published schema.

A custom model does not include the catalog's ready-made schema, shared vocabulary, or published `@context`, but you keep full control and can still use validation. The attributes are exactly the ones the data supports, named as the data reads, in the shapes the data has. Point Cassiopeia at a JSON Schema you author and it enforces your custom model just as it enforces a catalog model, checking either the simplified surface or the full normalized shape. [Example 27](examples/27-csv-custom-schema/example.md) validates a custom model against a hand-written key-values schema, and [example 28](examples/28-json-advanced-schema/example.md) validates one against a normalized schema that checks wrappers and metadata. The trade-off is the missing ready-made material, not reduced capability.

## Which to reach for

The choice depends on the case. Use this table as a starting point, not a rule:

| Use a Smart Data Model if | Use a custom model if |
| --- | --- |
| The entity will interoperate with other FIWARE or data-space producers and consumers. | The data is for internal or single-purpose use, where a shared vocabulary buys nothing. |
| A published model exists for the domain and fits the data. | No published model covers the domain, or the closest one is a poor fit. |
| You want a schema and `@context` ready-made, without authoring them. | The published model's shape would distort the data (see the tradeoffs below). |
| Lining up field-for-field with other datasets of the same type matters. | You need attributes, names, or types the published model does not allow. |

The two are not mutually exclusive within a run. The default validation mode lets an entity through when no schema describes its type, so a single manifest can mix published-model types, which are validated, with custom types, which are either unchecked or checked against a schema you supply. The [relationship-graph example](examples/11-csv-relationship-graph/example.md) does exactly that: it validates its Smart Data Model types while a schemaless `Country` entity passes through.

## Where Smart Data Models fall short

A published model is a good default when one genuinely fits. That qualification matters. The following are common reasons to choose a custom model. None is a criticism of the catalog. Each is a case where checking the fit is better than assuming it.

- **Coverage gaps.** The catalog is broad but not universal. Some domains have no model, including astronomy in the [exoplanet example](examples/20-csv-at-context/example.md), so a custom model is the only option. A missing catalog model does not mean giving up validation. You can author a JSON Schema for your model, and Cassiopeia enforces it just as it would a catalog schema, as [example 27](examples/27-csv-custom-schema/example.md) and [example 28](examples/28-json-advanced-schema/example.md) do.
- **A near-fit is still a misfit.** A published model that sits *near* the data can be tempting, but bending the data to a model that only approximately describes it produces a worse entity than modeling the data on its own terms. The useful question is not "is there a model close to this?" but "is this model what the data actually is?" A near-fit model is easy to choose by accident because it is already in the catalog. Modeling on the data's own terms also includes choices a schema may or may not require. For example, decoding a terse source code or bare number into a named enumerated value is good modeling either way. A named token reads and queries better than a raw code or magic number, whether or not a schema requires it. The [parking example](examples/03-geojson-smart-data-model/example.md) decodes under a published schema. The [world-ports example](examples/15-shapefile-enum-decoding/example.md) decodes with no schema because the value is in the modeling, not the validation.
- **Older models predate newer attribute types.** A published model can use only the attribute types that existed when it was written, and many describe only the three original core types: `Property`, `Relationship`, and `GeoProperty`. The newer NGSI-LD types, `VocabProperty`, `ListProperty`, `ListRelationship`, `LanguageProperty`, and `JsonProperty`, model many values more precisely. A model that predates them cannot express them, and its schema rejects them. The [list-relationship example](examples/12-csv-list-relationship/example.md) shows the issue: a `ListRelationship` describes a route's several aircraft more faithfully than the Flight model's single-object `hasAircraftModel`, so it does not validate against that model. Conformance and fidelity can diverge, and the modeler must decide which matters for a given consumer.
- **A catalog schema validates only the surface.** A Smart Data Model schema describes the simplified key-values form, so it checks bare values and nothing around them. It cannot see what lives in the normalized wrapper: a sub-attribute, the presence or shape of a temporal `observedAt`, or a `unitCode` that confirms a temperature carries `CEL` or `KEL` rather than `MTR`. Enforcing that layer requires a schema written for the normalized form, which the catalog does not ship. [Example 28](examples/28-json-advanced-schema/example.md) hand-authors one. A catalog schema alone leaves every qualifier and sub-attribute unchecked.
- **Naming is a JSON-LD detail, not a barrier.** A model may name an attribute differently from how the data naturally reads. That is not an obstacle to interoperability. In JSON-LD, the term is local and the IRI is global, so a custom `@context` can bind a natural attribute name to the model's canonical IRI, and consumers still resolve the same global term. Aliasing local names onto shared IRIs is the purpose of `@context`. A catalog *schema* keys on the JSON attribute name, so renaming trades schema-name conformance for natural naming while preserving the semantics. That is a separate decision from interoperability. A required field the source does not carry is harder, but synthesizing a value just to satisfy the model returns to the near-fit problem. If the data does not have the field, that is a fact about the data, not a gap to paper over.

A published model that is missing only a field or two does not necessarily need to become a custom model. You can add custom attributes alongside the model's own, and they will still validate because Smart Data Models schemas allow unlisted properties. Other consumers may not understand those attributes, however, so keep every model-owned attribute exactly as defined. Extend the model, but do not redefine it. If the source lacks a required field, do not invent a value just to satisfy the schema. That usually indicates that the published model is only a near fit and a custom model may be more honest.

Consider contributing upstream when an addition would be useful beyond your application. This applies to generally useful attributes, newer attribute types, broader enumerations, or an entire model for a domain the catalog does not cover. The catalog is community-governed and accepts proposals through issues or pull requests. Contributing a useful change turns a private workaround into shared vocabulary and saves other producers from solving the same modeling problem.

## How Cassiopeia relates to the choice

Cassiopeia stays neutral. It produces valid NGSI-LD for published and custom models alike, and the choice lives entirely in the mapping and run settings.

- **The `dataModel` field** names the model and, for a published one, locates its schema. A qualified name such as `dataModel.Weather/WeatherObserved` is looked up under its repository subdirectory. An unqualified name such as `BicycleCounter` is looked up at the schema folder root. Finding nothing there means the type has no catalog schema. The [validation guide](validation.md#where-schemas-come-from) covers the lookup in full.
- **Validation is opt-in per model and is not limited to the catalog.** Cassiopeia checks an entity whose type has a schema against it. In the default mode, it writes an entity whose type has no schema without checking it. To validate a custom model, point Cassiopeia at a schema you author, per input or for the whole run, without changing the mapping. See [custom schemas](validation.md#custom-schemas). The [validation modes](validation.md#validation-modes) decide how strict a failure or missing schema is.
- **The `@context` is delivered separately from the mapping.** A published model can have its context resolved from the catalog (`--context default`). A custom model's context can be authored as a local JSON-LD file and attached with `--context local`, or delivered by URL or a broker `Link` header. The [output guide](output.md#deliver-context) covers every delivery mode.
- **The Explorer and Wizard operate over models.** Before writing a mapping, the [Explorer](tui.md#explorer) browses a published model's schema, and the [Wizard](tui.md#wizard) authors a mapping against a chosen model. The Wizard also has a "create new data model" branch for custom models.

## Next steps

- [Output](output.md): learn where entities go, how they are serialized, and how a model's `@context` reaches the output.
- [NGSI-LD attribute types](ngsi-ld/attribute-types.md): the attribute types a model's attributes are built from.
- [Validation](validation.md): how a model's schema is located and applied, how a custom schema is supplied, and how strict a failure is.
- [Terminal interfaces](tui.md): browsing a model's schema and authoring a mapping against it.
- [Write a mapping](mapping.md): declaring a model and its attributes.
