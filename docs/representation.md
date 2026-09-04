# Representations

A representation is the shape an NGSI-LD entity takes when it is serialized. The same entity can be written in normalized, concise, or simplified form. All three carry the same information, but they use different amounts of structure. This page explains how Cassiopeia chooses the form it writes and the separate form it uses for validation.

For examples of all three representations and the NGSI-LD clauses that define them, see the [spec-level representation guide](ngsi-ld/representation.md).

## Writing is normalized by default

Cassiopeia writes **normalized** entities unless a run says otherwise. Normalized spells out every attribute's `type` and keeps all metadata, so it is the least ambiguous form for a broker, validator, or reader.

Two places can override the writer's representation for one run:

- the manifest `output.representation` field, and
- the `--writer-representation` flag on a command-line run.

Both accept `normalized`, `concise`, or `simplified`. When a manifest sets `output.representation`, it takes precedence over the command-line flag for a manifest run. When neither is set, the run writes normalized. The [output guide](output.md#select-a-representation) shows where to set the option and what each form looks like.

## Validation is simplified by default

Validation uses a different default on purpose. Cassiopeia checks entities in the **simplified (key-values)** representation because that is the shape a Smart Data Model JSON Schema describes. The schema constrains `temperature` as the bare number `23.0`, not as the normalized Property wrapper with its `type` and `value` members. Validating the normalized form against a key-values schema would report failures caused only by structure. The [validation guide](validation.md) covers this in full.

Set the validation representation per run in either of the same two places as the validation mode:

- the manifest `output.validation.representation` field, and
- the `--validation-representation` flag (`normalized`, `concise`, or `simplified`) on a command-line run.

Use `--validation-skip-null` or `output.validation.skipNull` (`skip` or `include`) alongside it. A command-line flag overrides the manifest, which overrides the simplified default.

The whole validation cluster, including mode, custom schema, representation, skip-null, and report path, lives under `output.validation`. The representation used for validation is separate from the representation Cassiopeia writes.

## The two settings are independent

Selecting a representation changes serialization only. It does not change the entity assembled by the mapping, and the writer's representation does not change what validation checks. A run can write concise while validation checks simplified, or write normalized while validation checks normalized. Choose the output representation for the system that receives the entities. Leave validation on simplified unless the target schema describes another form.

~~~bash
cassiopeia map \
    --input data.csv \
    --mapping mapping.json5 \
    --output out \
    --writer-representation concise \
    --validation-representation simplified
~~~

This run writes concise entities to `out` and checks each one against its schema in the key-values form the schema describes.

## Next steps

- [Validation](validation.md): learn about the modes and schema lookup used with the validation representation.
- [Output](output.md#serialize-entities): select the written representation and see each form.
- [NGSI-LD data representations](ngsi-ld/representation.md): see what each form looks like at the specification level.
