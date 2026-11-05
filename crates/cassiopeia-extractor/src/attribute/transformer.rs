use crate::{attribute::refusal::AttributeRefusal, required_value::RequiredValue};
use cassiopeia_geometry::{policy::GeometryPolicy, target::GeometryTarget};
use cassiopeia_ir::attribute_content::AttributeContent;
use cassiopeia_mapping::conversion::{
    Conversion,
    json_conversion::JsonConversion,
    property_conversion::PropertyConversion,
    value_reading::{TemporalReading, ValueReading},
};
use cassiopeia_ngsi_ld::value::{
    json_payload::JsonPayload,
    omitted_nulls::OmittedNulls,
    parsing::parse_decimal,
    types::{TemporalValue, Value, ValueObject},
};
use compact_str::CompactString;
use serde_json::Value as JsonValue;
use smallvec::SmallVec;

/// The raw source values read for one attribute declaration, one per source template.
///
/// A declaration names a single source in the overwhelmingly common case, so the inline capacity of
/// one keeps every leaf, language entry, instance, and metadata sub-attribute of every record off the
/// heap, where a `Vec` would cost one malloc/free pair per resolved declaration.
pub(crate) type SourceParts = SmallVec<[JsonValue; 1]>;

/// Converts the raw source values read for an attribute into its content.
pub(crate) struct Transformer;

impl Transformer {
    /// Applies a conversion to the source values collected for one attribute.
    ///
    /// The conversion was resolved when the mapping loaded (see [`Conversion::resolve`]). A
    /// `JsonProperty` naming no transformation keeps what the source held as raw JSON, and one under
    /// `json` does so after reading each text part as the JSON it is written as. `array`, and a
    /// `ListProperty`, aggregate all parts; every other conversion first merges the parts into one
    /// intermediate value and then reads it as the value its type holds.
    ///
    /// Reading a source into an NGSI-LD Value leaves out every null nested in it, since none can sit
    /// in a Value (ETSI GS CIM 009 v1.9.1 clause 4.5.0), and counts each in `omitted`. Raw JSON keeps
    /// its nulls.
    ///
    /// `Ok(None)` means the source carried no value. A geometry reading routes through the geometry
    /// lattice, where the source may carry no geometry at all, a geometry a `GeoProperty` cannot
    /// hold, or one this mapping did not authorise converting; a temporal reading refuses text that
    /// reads as no supported spelling of a date-time; a boolean, integer, or float reading refuses a
    /// value that spells no such scalar; an object or JSON reading, and a `JsonProperty` under
    /// `json`, refuse text that does not read as the JSON they ask for; a `LanguageProperty` refuses
    /// a value that is no object of tagged text; and raw JSON that is no object or array of objects is
    /// no `JsonProperty` payload. Text and `array` readings can use any value and never refuse one.
    ///
    /// # Errors
    /// Returns the [`AttributeRefusal`] naming what the attribute could not take.
    pub(crate) fn apply(
        parts: SourceParts,
        conversion: Conversion,
        geometry: Option<&GeometryPolicy>,
        omitted: &mut OmittedNulls,
    ) -> Result<Option<AttributeContent>, AttributeRefusal> {
        let value = match conversion {
            Conversion::JsonProperty(JsonConversion::Verbatim) => return Ok(Self::keep_as_read(parts)?.map(AttributeContent::Json)),
            Conversion::JsonProperty(JsonConversion::Json) => return Ok(Self::keep_as_read(Self::decode(parts)?)?.map(AttributeContent::Json)),
            Conversion::JsonProperty(JsonConversion::Array) | Conversion::ListProperty | Conversion::Property(PropertyConversion::Array) => {
                Self::aggregate(parts, omitted)
            }
            Conversion::JsonProperty(JsonConversion::Object) => Self::read(parts, ValueReading::Object, geometry, omitted)?,
            Conversion::Property(PropertyConversion::One(reading)) => Self::read(parts, reading, geometry, omitted)?,
            Conversion::GeoProperty(target) => Self::read(parts, ValueReading::Geometry(target), geometry, omitted)?,
            Conversion::LanguageProperty => Self::merge_text(parts, omitted).map_or(Ok(None), |value| Self::coerce_language_map(value, omitted))?,
            Conversion::LanguageText | Conversion::VocabProperty => Self::read(parts, ValueReading::Text, geometry, omitted)?,
            // A relationship's object is built by the expander from its `source` and `target`, so
            // there is no value of its own to read here.
            Conversion::Relationship => None,
        };

        Ok(value.map(AttributeContent::Value))
    }

    /// Applies a conversion to the object an attribute's nested `mappings` assembled.
    ///
    /// The object converts exactly as it would read from one source: kept as assembled when the
    /// declaration names no transformation (a `Property` reads it as an object, and a
    /// `JsonProperty` keeps it as it is), and otherwise through the same reading a value read from a
    /// source goes through, so `string` writes its compact JSON text and `array` a list of the one
    /// object. A `JsonProperty` under `json` keeps it as well, since an assembled object holds no
    /// text to decode.
    ///
    /// # Errors
    /// Returns the [`AttributeRefusal`] naming what the attribute could not take: under a geometry
    /// reading, a geometry the declared type cannot hold, or [`RequiredValue::Geometry`] quoting an
    /// object that spells no geometry at all; under any other reading, what that reading refuses.
    pub(crate) fn apply_to_object(
        object: ValueObject,
        conversion: Conversion,
        geometry: Option<&GeometryPolicy>,
        omitted: &mut OmittedNulls,
    ) -> Result<Option<AttributeContent>, AttributeRefusal> {
        let object = Value::Object(Box::new(object));
        let value = match conversion {
            Conversion::JsonProperty(JsonConversion::Verbatim | JsonConversion::Json) | Conversion::LanguageProperty => Some(object),
            Conversion::JsonProperty(JsonConversion::Array) | Conversion::ListProperty | Conversion::Property(PropertyConversion::Array) => {
                Some(Self::as_list(object))
            }
            Conversion::JsonProperty(JsonConversion::Object) => Self::coerce_merged(object, ValueReading::Object, geometry, omitted)?,
            Conversion::Property(PropertyConversion::One(reading)) => Self::coerce_merged(object, reading, geometry, omitted)?,
            Conversion::GeoProperty(target) => Self::coerce_merged(object, ValueReading::Geometry(target), geometry, omitted)?,
            Conversion::LanguageText | Conversion::VocabProperty => Self::coerce_merged(object, ValueReading::Text, geometry, omitted)?,
            // A relationship's object is built by the expander from its `source` and `target`, so
            // there is no value of its own to read here.
            Conversion::Relationship => None,
        };

        Ok(value.map(AttributeContent::Value))
    }

    /// Keeps the parts as the source held them, as raw JSON: a lone part is its own payload, several
    /// are an array of the parts in order.
    ///
    /// Nothing is coerced, stringified, or filtered by JSON type, because the payload is raw JSON
    /// that NGSI-LD never interprets (ETSI GS CIM 009 v1.9.1 clause 4.5.24.2), so a null nested in a
    /// part is kept. A part that is null is a field the record does not carry and is left out, and
    /// when no part is left there is no payload.
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::JsonPayload`] when what is left is not "a raw JSON object (or array
    /// of objects)" (clause 4.5.24.2).
    fn keep_as_read(parts: SourceParts) -> Result<Option<JsonPayload>, AttributeRefusal> {
        let mut parts = parts.into_iter().filter(|part| !part.is_null()).collect::<SourceParts>();
        let json = match parts.len() {
            0 => return Ok(None),
            1 => parts.swap_remove(0),
            _ => JsonValue::Array(parts.into_vec()),
        };

        Ok(Some(JsonPayload::try_from(json)?))
    }

    /// Reads every text part as the JSON it is written as, keeping every other part as it is.
    ///
    /// Blank text is a field the record does not carry, the way CSV spells an empty cell, so it
    /// becomes null and is left out with the other absent parts.
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::UnusableValue`] quoting the first text part that is not JSON.
    fn decode(parts: SourceParts) -> Result<SourceParts, AttributeRefusal> {
        parts
            .into_iter()
            .map(|part| match part {
                JsonValue::String(text) => Ok(Self::read_json(&text, RequiredValue::Json)?.unwrap_or(JsonValue::Null)),
                JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_) => Ok(part),
            })
            .collect()
    }

    /// Reads text as the JSON it is written as (RFC 8259 clause 2), or as nothing when it is blank.
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::UnusableValue`] quoting the text, as not the `required` value,
    /// when it is not JSON.
    fn read_json(text: &str, required: RequiredValue) -> Result<Option<JsonValue>, AttributeRefusal> {
        if text.trim().is_empty() {
            return Ok(None);
        }
        serde_json::from_str(text).map(Some).map_err(|_| AttributeRefusal::unusable(required, text))
    }

    /// Reads the source parts as one value, the way `reading` names.
    ///
    /// A geometry reading keeps the parts' array structure so their order, the coordinate order,
    /// survives (see [`Transformer::coerce_geometry_parts`]). A scalar reading takes the first part
    /// present, and every other reading merges the parts into text; the merged value is then read
    /// like any other (see [`Transformer::coerce_merged`]).
    fn read(
        parts: SourceParts,
        reading: ValueReading,
        geometry: Option<&GeometryPolicy>,
        omitted: &mut OmittedNulls,
    ) -> Result<Option<Value>, AttributeRefusal> {
        let merged = match reading {
            ValueReading::Geometry(target) => return Self::coerce_geometry_parts(parts, target, geometry, omitted),
            ValueReading::Boolean | ValueReading::Integer | ValueReading::Float => Self::merge_first(parts, omitted),
            ValueReading::Text | ValueReading::Object | ValueReading::Json | ValueReading::Temporal(_) => Self::merge_text(parts, omitted),
        };

        merged.map_or(Ok(None), |value| Self::coerce_merged(value, reading, geometry, omitted))
    }

    /// Reads one value, already merged from the source parts or assembled from nested `mappings`, the
    /// way `reading` names.
    fn coerce_merged(
        value: Value,
        reading: ValueReading,
        geometry: Option<&GeometryPolicy>,
        omitted: &mut OmittedNulls,
    ) -> Result<Option<Value>, AttributeRefusal> {
        match reading {
            ValueReading::Boolean => Self::coerce_scalar(&value, RequiredValue::Boolean, |value| value.to_boolean().map(Value::from)),
            ValueReading::Integer => Self::coerce_scalar(&value, RequiredValue::Integer, Self::coerce_integer),
            ValueReading::Float => Self::coerce_scalar(&value, RequiredValue::Float, Self::coerce_float),
            ValueReading::Text => Ok(Some(Self::coerce_string(value))),
            ValueReading::Object => Self::coerce_object(value, omitted),
            ValueReading::Json => Self::coerce_json(value, omitted),
            ValueReading::Temporal(temporal) => Self::coerce_temporal(&value, temporal),
            ValueReading::Geometry(target) => Self::coerce_geometry(&value, target, geometry).map(Some),
        }
    }

    /// A value as a list: a value that is already an array is the list, and any other value is its
    /// one element, as a lone part of either kind is aggregated.
    fn as_list(value: Value) -> Value {
        match value {
            list @ Value::Array(_) => list,
            other @ (Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Object(_)) => {
                Value::Array(vec![other])
            }
        }
    }

    /// Keeps the merged value when it is an object, the `languageMap` a `LanguageProperty` holds,
    /// reading text as the object it is written as.
    ///
    /// Which of its entries enter the map is decided when the content is typed as the declared kind:
    /// only an entry keyed by a BCP-47 tag and holding text does (ETSI GS CIM 009 v1.9.1 clause
    /// 4.5.18). Blank text yields no value. A null member of an object read from text is left out
    /// and counted in `omitted`, as it is in an object read structured.
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::UnusableValue`] quoting a value that is no object as not
    /// [`RequiredValue::LanguageMap`]: text as written when it is not JSON or is JSON but no object,
    /// any other value as its rendering.
    fn coerce_language_map(value: Value, omitted: &mut OmittedNulls) -> Result<Option<Value>, AttributeRefusal> {
        match value {
            Value::String(text) => match Self::read_json(&text, RequiredValue::LanguageMap)? {
                Some(object @ JsonValue::Object(_)) => Ok(Value::from_json(object, omitted)),
                Some(JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) | JsonValue::Array(_)) => {
                    Err(AttributeRefusal::unusable(RequiredValue::LanguageMap, &text))
                }
                None => Ok(None),
            },
            object @ Value::Object(_) => Ok(Some(object)),
            other @ (Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_)) => {
                Err(AttributeRefusal::unusable(RequiredValue::LanguageMap, &other.to_string()))
            }
        }
    }

    /// Flattens the parts into one array, dropping absent parts and splicing in any nested arrays.
    ///
    /// A part is absent when it is null or blank text: CSV and spreadsheet sources spell an empty
    /// field as an empty string, so a blank is a value the record does not carry, and keeping it would
    /// emit `[""]` for an empty cell. A text part that reads as a JSON array is spliced in like an
    /// array part, since a CSV source can carry an array only as its text; any other text, such as
    /// `BS IN`, is one element. Only whole parts are judged that way; the elements of an array part
    /// are the source's own data and are spliced in as they are, except that a null element is left
    /// out and counted in `omitted`, since a `valueList` is an ordered array of NGSI-LD Values (ETSI
    /// GS CIM 009 v1.9.1 clause 4.5.21.2) and null is none (clause 3.1).
    fn aggregate(parts: SourceParts, omitted: &mut OmittedNulls) -> Option<Value> {
        let mut result = Vec::new();
        for part in parts {
            match part {
                JsonValue::Array(array) => result.extend(array),
                JsonValue::Null => {}
                JsonValue::String(text) if text.trim().is_empty() => {}
                JsonValue::String(text) => match serde_json::from_str::<JsonValue>(&text).ok() {
                    Some(JsonValue::Array(array)) => result.extend(array),
                    Some(JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) | JsonValue::Object(_)) | None => {
                        result.push(JsonValue::String(text));
                    }
                },
                JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Object(_) => result.push(part),
            }
        }

        match Value::from_json(JsonValue::Array(result), omitted) {
            Some(Value::Array(items)) if items.is_empty() => None,
            aggregated => aggregated,
        }
    }

    /// Merges parts for a scalar target: a lone part keeps its type, several take the first non-null.
    fn merge_first(mut parts: SourceParts, omitted: &mut OmittedNulls) -> Option<Value> {
        if parts.len() == 1 {
            return Value::from_json(parts.swap_remove(0), omitted);
        }

        let first = parts.into_iter().find(|part| !part.is_null())?;
        Value::from_json(first, omitted)
    }

    /// Merges parts for a text target: a lone part keeps its type, several are concatenated.
    fn merge_text(mut parts: SourceParts, omitted: &mut OmittedNulls) -> Option<Value> {
        if parts.len() == 1 {
            return Value::from_json(parts.swap_remove(0), omitted);
        }

        let combined = parts
            .iter()
            .filter(|part| !part.is_null())
            .map(|part| match part {
                JsonValue::String(text) => text.clone(),
                JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_) => part.to_string(),
            })
            .collect::<String>();

        if combined.is_empty() {
            None
        } else {
            Some(Value::String(CompactString::from(combined)))
        }
    }

    /// Merges parts for a geometry target: a lone part keeps its type, several keep their array
    /// structure so their order (the coordinate order) survives.
    ///
    /// A part that is null is a field the record does not carry, not a null inside the source's own
    /// data, so it is left out without being counted.
    fn merge_structured(mut parts: SourceParts, omitted: &mut OmittedNulls) -> Option<Value> {
        if parts.len() == 1 {
            return Value::from_json(parts.swap_remove(0), omitted);
        }

        let present = parts.into_iter().filter(|part| !part.is_null()).collect::<Vec<JsonValue>>();
        if present.is_empty() {
            return None;
        }
        Value::from_json(JsonValue::Array(present), omitted)
    }

    /// Reads the merged value as the scalar a transformation names, through `read`.
    ///
    /// A blank value resolves to no value, as an absent source does: CSV and spreadsheet sources
    /// spell an empty field as an empty string, so a blank is a value the record does not carry
    /// rather than one it carries wrongly. Any other value that `read` finds no scalar in is
    /// refused, text and non-text alike (text such as `abc` under `integer`, an array, an object),
    /// because a reading that vanishes unannounced is a value the published entity silently lacks.
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::UnusableValue`] quoting the value as not the `required` scalar:
    /// text as written, any other value as its rendering.
    fn coerce_scalar(value: &Value, required: RequiredValue, read: impl FnOnce(&Value) -> Option<Value>) -> Result<Option<Value>, AttributeRefusal> {
        if let Some(scalar) = read(value) {
            return Ok(Some(scalar));
        }

        value.reported_text().map_or(Ok(None), |text| Err(AttributeRefusal::unusable(required, &text)))
    }

    /// Coerces the merged value to an integer, parsing numeric strings explicitly.
    ///
    /// Text is read without the whitespace around it, as a padded CSV cell such as ` 12 ` spells
    /// the number it surrounds. A float-shaped string truncates toward zero through the same
    /// conversion the value model uses, so a value outside the `i64` range is no integer rather than
    /// saturating.
    fn coerce_integer(value: &Value) -> Option<Value> {
        match value {
            Value::String(text) => {
                let text = text.trim();
                text.parse::<i64>()
                    .ok()
                    .or_else(|| {
                        text.parse::<f64>()
                            .ok()
                            .and_then(|float| Value::try_from(float).ok())
                            .and_then(|parsed| parsed.to_integer())
                    })
                    .map(Value::from)
            }
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
                value.to_integer().map(Value::from)
            }
        }
    }

    /// Coerces the merged value to a float, accepting a comma as a decimal separator.
    ///
    /// Text is read without the whitespace around it, as [`Transformer::coerce_integer`] reads it.
    fn coerce_float(value: &Value) -> Option<Value> {
        match value {
            Value::String(text) => parse_decimal(text.trim()).and_then(|float| Value::try_from(float).ok()),
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
                value.to_float().and_then(|float| Value::try_from(float).ok())
            }
        }
    }

    /// Coerces the merged value to a string.
    fn coerce_string(value: Value) -> Value {
        match &value {
            Value::String(_) => value,
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
                Value::String(CompactString::from(value.to_string()))
            }
        }
    }

    /// Keeps the merged value when it is a non-empty JSON object, reading text as the object it is
    /// written as.
    ///
    /// Blank text and an empty object yield no value. A null member of an object read from text is
    /// left out and counted in `omitted`, as it is in an object read structured.
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::UnusableValue`] quoting a value that is no object: text as written
    /// when it is not JSON or is JSON but no object, any other value as its rendering.
    fn coerce_object(value: Value, omitted: &mut OmittedNulls) -> Result<Option<Value>, AttributeRefusal> {
        let value = match value {
            Value::String(text) => match Self::read_json(&text, RequiredValue::JsonObject)? {
                Some(object @ JsonValue::Object(_)) => Value::from_json(object, omitted),
                Some(JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) | JsonValue::Array(_)) => {
                    return Err(AttributeRefusal::unusable(RequiredValue::JsonObject, &text));
                }
                None => None,
            },
            object @ Value::Object(_) => Some(object),
            other @ (Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_)) => {
                return Err(AttributeRefusal::unusable(RequiredValue::JsonObject, &other.to_string()));
            }
        };

        Ok(value.filter(|value| matches!(value, Value::Object(object) if !object.is_empty())))
    }

    /// Reads merged text as the JSON value it is written as, and keeps any other value as it is.
    ///
    /// Blank text, and text spelling `null`, yield no value. A null nested in what the text holds is
    /// left out and counted in `omitted`, since none can sit in a Value (ETSI GS CIM 009 v1.9.1
    /// clause 4.5.0).
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::UnusableValue`] quoting text that is not JSON.
    fn coerce_json(value: Value, omitted: &mut OmittedNulls) -> Result<Option<Value>, AttributeRefusal> {
        match value {
            Value::String(text) => Ok(Self::read_json(&text, RequiredValue::Json)?.and_then(|json| Value::from_json(json, omitted))),
            other @ (Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_)) => Ok(Some(other)),
        }
    }

    /// Reads the source parts as the geometry the transformation names, under the mapping's policy.
    ///
    /// The parts carry no value when every one of them is null or blank text: CSV and spreadsheet
    /// sources spell an empty field as an empty string, so an empty cell, or a pair of empty
    /// coordinate cells, is a value the record does not carry, and naming every one would drown the
    /// run. Any other parts are merged and read (see [`Transformer::coerce_geometry`]).
    ///
    /// # Errors
    /// Returns the [`AttributeRefusal`] naming why the merged value is no geometry the attribute can
    /// hold.
    fn coerce_geometry_parts(
        parts: SourceParts,
        target: GeometryTarget,
        policy: Option<&GeometryPolicy>,
        omitted: &mut OmittedNulls,
    ) -> Result<Option<Value>, AttributeRefusal> {
        if parts.iter().all(Self::is_absent) {
            return Ok(None);
        }

        Self::merge_structured(parts, omitted).map_or(Ok(None), |merged| Self::coerce_geometry(&merged, target, policy).map(Some))
    }

    /// Whether a source part is a field the record does not carry: null, or blank text.
    fn is_absent(part: &JsonValue) -> bool {
        match part {
            JsonValue::Null => true,
            JsonValue::String(text) => text.trim().is_empty(),
            JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_) => false,
        }
    }

    /// Reads a value that is there as the geometry the transformation names, under the mapping's
    /// policy.
    ///
    /// The value is there, so one that spells no geometry at all is refused rather than left out,
    /// the way unreadable text under `datetime` is: a `GeoProperty` that vanishes unannounced is
    /// what leaves a published entity without its location. A geometry assembled from nested
    /// `mappings` is read here too, with no source parts left to merge (see
    /// [`Transformer::apply_to_object`]).
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::UnusableValue`] quoting a value that spells no geometry, text as
    /// written and any other value as its rendering, or [`AttributeRefusal::Geometry`] naming why
    /// the geometry it spells cannot become the declared type.
    fn coerce_geometry(value: &Value, target: GeometryTarget, policy: Option<&GeometryPolicy>) -> Result<Value, AttributeRefusal> {
        let policy = policy.copied().unwrap_or_default();
        match value.to_geometry(target, &policy)? {
            Some(geometry) => Ok(Value::Geospatial(Box::new(geometry))),
            None => Err(AttributeRefusal::unusable(
                RequiredValue::Geometry,
                &value.as_str().map_or_else(|| value.to_string(), str::to_owned),
            )),
        }
    }

    /// Coerces the merged value to a temporal value of the given kind.
    ///
    /// A blank value resolves to no value, as an absent source does: CSV and spreadsheet sources
    /// spell an empty field as an empty string, so a blank is a value the record does not carry
    /// rather than one it carries wrongly, and naming every empty cell of a column would drown the
    /// run. Any other value that is there and will not read is refused instead, text and non-text
    /// alike (a number too small to be an epoch, a boolean, an array), because a timestamp that
    /// vanishes unannounced is what leaves a published entity with nothing anchoring it in time.
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::UnreadableTimestamp`] quoting the value that would not read: text
    /// as written, any other value as its rendering.
    fn coerce_temporal(value: &Value, kind: TemporalReading) -> Result<Option<Value>, AttributeRefusal> {
        if let Some(datetime) = value.try_parse_datetime() {
            let temporal = match kind {
                TemporalReading::DateTime => TemporalValue::DateTime(datetime),
                TemporalReading::Date => TemporalValue::Date(datetime),
                TemporalReading::Time => TemporalValue::Time(datetime),
            };
            return Ok(Some(Value::Temporal(temporal)));
        }

        match value.as_str().map(str::trim) {
            Some("") => Ok(None),
            Some(text) => Err(AttributeRefusal::UnreadableTimestamp { text: Box::from(text) }),
            None => Err(AttributeRefusal::UnreadableTimestamp {
                text: value.to_string().into_boxed_str(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            refusal::AttributeRefusal,
            transformer::{SourceParts, Transformer},
        },
        required_value::RequiredValue,
    };
    use cassiopeia_geometry::{
        error::GeometryError,
        geometry::{GeometryKind, NgsiLdGeometry},
        policy::GeometryPolicy,
        strategy::ConversionStrategy,
    };
    use cassiopeia_ir::attribute_content::AttributeContent;
    use cassiopeia_mapping::{
        conversion::{
            Conversion,
            declaration_shape::{DeclarationShape, ValueOrigin},
            json_conversion::JsonConversion,
        },
        transformation::Transformation,
    };
    use cassiopeia_ngsi_ld::{
        entity::attribute::NgsiLdAttributeKind,
        value::{
            json_payload_error::{JsonPayloadError, JsonShape},
            omitted_nulls::OmittedNulls,
            types::{Number, Value, ValueObject},
        },
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use smallvec::smallvec;

    const SOURCED: DeclarationShape = DeclarationShape::Value(ValueOrigin::Sourced);

    /// Applies a conversion with a fresh null count, which these tests do not inspect.
    fn apply(parts: SourceParts, conversion: Conversion, policy: Option<&GeometryPolicy>) -> Result<Option<AttributeContent>, AttributeRefusal> {
        Transformer::apply(parts, conversion, policy, &mut OmittedNulls::default())
    }

    /// Applies a conversion expected to succeed with an NGSI-LD Value or with no value.
    fn converted(parts: SourceParts, conversion: Conversion, policy: Option<&GeometryPolicy>) -> Option<Value> {
        match apply(parts, conversion, policy).unwrap() {
            Some(AttributeContent::Value(value)) => Some(value),
            Some(other @ (AttributeContent::Json(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_))) => panic!("{other:?}, not a value"),
            None => None,
        }
    }

    /// Applies a conversion expected to succeed with raw JSON or with no value.
    fn payload(parts: SourceParts, conversion: Conversion) -> Option<JsonValue> {
        match apply(parts, conversion, None).unwrap() {
            Some(AttributeContent::Json(payload)) => Some(JsonValue::from(payload)),
            Some(other @ (AttributeContent::Value(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_))) => panic!("{other:?}, not raw JSON"),
            None => None,
        }
    }

    /// Applies a conversion and returns the nulls it left out of the value.
    fn omitted(parts: SourceParts, conversion: Conversion) -> (Option<Value>, OmittedNulls) {
        let mut omitted = OmittedNulls::default();
        let content = Transformer::apply(parts, conversion, None, &mut omitted).unwrap();
        let value = match content {
            Some(AttributeContent::Value(value)) => Some(value),
            Some(other @ (AttributeContent::Json(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_))) => panic!("{other:?}, not a value"),
            None => None,
        };
        (value, omitted)
    }

    /// The object `{"street": "Ring"}`, as nested `mappings` would assemble it.
    fn street_object() -> ValueObject {
        match Value::try_from(json!({"street": "Ring"})).unwrap() {
            Value::Object(object) => *object,
            other @ (Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_)) => {
                panic!("{other:?}, not an object")
            }
        }
    }

    /// Applies a conversion to the assembled street object with a fresh null count.
    fn apply_to_street(conversion: Conversion) -> Result<Option<AttributeContent>, AttributeRefusal> {
        Transformer::apply_to_object(street_object(), conversion, None, &mut OmittedNulls::default())
    }

    #[test]
    fn an_assembled_object_kept_as_assembled_or_decoded_stays_the_object() {
        for conversion in [
            Conversion::JsonProperty(JsonConversion::Verbatim),
            Conversion::JsonProperty(JsonConversion::Json),
        ] {
            assert_eq!(
                apply_to_street(conversion),
                Ok(Some(AttributeContent::Value(Value::try_from(json!({"street": "Ring"})).unwrap()))),
                "{conversion:?}"
            );
        }
    }

    #[test]
    fn an_assembled_object_under_the_string_transformation_becomes_its_compact_json_text() {
        assert_eq!(
            apply_to_street(property(Transformation::String)),
            Ok(Some(AttributeContent::Value(Value::String(r#"{"street":"Ring"}"#.into()))))
        );
    }

    #[test]
    fn an_assembled_object_under_the_array_transformation_becomes_a_list_of_the_object() {
        assert_eq!(
            apply_to_street(property(Transformation::Array)),
            Ok(Some(AttributeContent::Value(Value::try_from(json!([{"street": "Ring"}])).unwrap())))
        );
    }

    #[test]
    fn an_assembled_object_under_the_object_and_json_transformations_stays_the_object() {
        for transformation in [Transformation::Object, Transformation::Json] {
            assert_eq!(
                apply_to_street(property(transformation)),
                Ok(Some(AttributeContent::Value(Value::try_from(json!({"street": "Ring"})).unwrap()))),
                "{transformation}"
            );
        }
    }

    #[test]
    fn an_assembled_object_spelling_no_geometry_under_a_geometry_transformation_is_refused_quoting_it() {
        for transformation in [Transformation::Geometry, Transformation::Point, Transformation::MultiPolygon] {
            assert_eq!(
                apply_to_street(property(transformation)),
                Err(AttributeRefusal::unusable(RequiredValue::Geometry, r#"{"street":"Ring"}"#)),
                "{transformation}"
            );
        }
    }

    #[test]
    fn an_assembled_point_under_the_multipoint_transformation_becomes_a_multipoint() {
        let point = match Value::try_from(json!({"type": "Point", "coordinates": [16.37, 48.2]})).unwrap() {
            Value::Object(object) => *object,
            other @ (Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_)) => {
                panic!("{other:?}, not an object")
            }
        };

        let content = Transformer::apply_to_object(point, property(Transformation::MultiPoint), None, &mut OmittedNulls::default());

        let expected: NgsiLdGeometry = serde_json::from_value(json!({"type": "MultiPoint", "coordinates": [[16.37, 48.2]]})).unwrap();
        assert_eq!(content, Ok(Some(AttributeContent::Value(Value::Geospatial(Box::new(expected))))));
    }

    #[test]
    fn a_lone_numeric_part_keeps_its_type_under_a_float_transformation() {
        let value = converted(smallvec![json!(25.5)], property(Transformation::Float), None);

        assert_eq!(value, Some(Value::try_from(25.5).unwrap()));
    }

    #[test]
    fn a_numeric_string_is_parsed_to_an_integer() {
        let value = converted(smallvec![json!("7")], property(Transformation::Integer), None);

        assert_eq!(value, Some(Value::Number(Number::Integer(7))));
    }

    #[test]
    fn a_float_string_is_truncated_to_an_integer() {
        let value = converted(smallvec![json!("7.9")], property(Transformation::Integer), None);

        assert_eq!(value, Some(Value::Number(Number::Integer(7))));
    }

    #[test]
    fn a_comma_decimal_string_parses_as_a_float() {
        let value = converted(smallvec![json!("1,5")], property(Transformation::Float), None);

        assert_eq!(value, Some(Value::try_from(1.5).unwrap()));
    }

    #[test]
    fn several_string_parts_are_concatenated() {
        let value = converted(smallvec![json!("Station-"), json!(42)], property(Transformation::String), None);

        assert_eq!(value, Some(Value::String("Station-42".into())));
    }

    #[test]
    fn string_parts_past_the_inline_capacity_are_concatenated_in_order() {
        // Four parts spill the inline capacity of one onto the heap; order must survive the spill.
        let value = converted(
            smallvec![json!("Station-"), json!(42), json!("/"), json!("north")],
            property(Transformation::String),
            None,
        );

        assert_eq!(value, Some(Value::String("Station-42/north".into())));
    }

    /// The conversion an attribute of `kind` declared without a transformation resolves to.
    /// The conversion a declaration of `kind` read from a source and naming no transformation
    /// loads with.
    fn default_of(kind: NgsiLdAttributeKind) -> Conversion {
        Conversion::resolve(kind, None, SOURCED).unwrap()
    }

    /// The conversion a `Property` read from a source under `transformation` loads with.
    fn property(transformation: Transformation) -> Conversion {
        Conversion::resolve(NgsiLdAttributeKind::Property, Some(transformation), SOURCED).unwrap()
    }

    #[test]
    fn a_boolean_part_under_the_property_default_becomes_its_text() {
        let value = converted(smallvec![json!(true)], default_of(NgsiLdAttributeKind::Property), None);

        assert_eq!(value, Some(Value::String("true".into())));
    }

    #[test]
    fn an_array_part_under_a_string_transformation_becomes_its_compact_json_text() {
        let value = converted(smallvec![json!(["BS", "IN"])], property(Transformation::String), None);

        assert_eq!(value, Some(Value::String(r#"["BS","IN"]"#.into())));
    }

    #[test]
    fn an_array_part_under_the_property_default_becomes_its_compact_json_text() {
        let value = converted(smallvec![json!(["BS", "IN"])], default_of(NgsiLdAttributeKind::Property), None);

        assert_eq!(value, Some(Value::String(r#"["BS","IN"]"#.into())));
    }

    #[test]
    fn an_object_part_under_a_string_transformation_becomes_its_compact_json_text() {
        let value = converted(smallvec![json!({"a": 1, "b": ["x"]})], property(Transformation::String), None);

        assert_eq!(value, Some(Value::String(r#"{"a":1,"b":["x"]}"#.into())));
    }

    #[test]
    fn an_object_part_under_the_property_default_becomes_its_compact_json_text() {
        let value = converted(smallvec![json!({"a": 1, "b": ["x"]})], default_of(NgsiLdAttributeKind::Property), None);

        assert_eq!(value, Some(Value::String(r#"{"a":1,"b":["x"]}"#.into())));
    }

    #[test]
    fn a_whole_float_part_under_the_property_default_becomes_its_shortest_text() {
        let value = converted(smallvec![json!(3.0)], default_of(NgsiLdAttributeKind::Property), None);

        assert_eq!(value, Some(Value::String("3".into())));
    }

    #[test]
    fn an_array_part_under_the_list_property_default_becomes_the_list_itself() {
        let value = converted(smallvec![json!(["BS", "IN"])], default_of(NgsiLdAttributeKind::ListProperty), None);

        assert_eq!(value, Some(Value::try_from(json!(["BS", "IN"])).unwrap()));
    }

    #[test]
    fn a_scalar_part_under_the_list_property_default_becomes_a_one_element_list() {
        let value = converted(smallvec![json!("BS")], default_of(NgsiLdAttributeKind::ListProperty), None);

        assert_eq!(value, Some(Value::try_from(json!(["BS"])).unwrap()));
    }

    #[test]
    fn a_blank_or_null_part_under_the_list_property_default_is_no_value() {
        for part in [json!(""), json!("  "), json!(null)] {
            let value = converted(smallvec![part], default_of(NgsiLdAttributeKind::ListProperty), None);

            assert_eq!(value, None, "{value:?}");
        }
    }

    #[test]
    fn an_object_part_under_the_json_property_default_is_kept_as_it_is() {
        let payload = payload(smallvec![json!({"a": 1, "b": ["x"]})], default_of(NgsiLdAttributeKind::JsonProperty));

        assert_eq!(payload, Some(json!({"a": 1, "b": ["x"]})));
    }

    #[test]
    fn a_null_nested_in_a_json_property_part_is_kept() {
        let payload = payload(
            smallvec![json!({"maybe": null, "list": [1, null]})],
            default_of(NgsiLdAttributeKind::JsonProperty),
        );

        assert_eq!(payload, Some(json!({"maybe": null, "list": [1, null]})));
    }

    #[test]
    fn an_array_of_objects_under_the_json_property_default_is_kept_as_it_is() {
        let payload = payload(smallvec![json!([{"a": 1}, {"b": 2}])], default_of(NgsiLdAttributeKind::JsonProperty));

        assert_eq!(payload, Some(json!([{"a": 1}, {"b": 2}])));
    }

    #[test]
    fn a_scalar_part_under_the_json_property_default_is_refused() {
        assert_eq!(
            apply(smallvec![json!(3.5)], default_of(NgsiLdAttributeKind::JsonProperty), None),
            Err(AttributeRefusal::JsonPayload(JsonPayloadError::NotAnObject { found: JsonShape::Number }))
        );
    }

    #[test]
    fn a_null_part_under_the_json_property_default_is_no_value() {
        assert_eq!(payload(smallvec![json!(null)], default_of(NgsiLdAttributeKind::JsonProperty)), None);
    }

    #[test]
    fn several_parts_under_the_json_property_default_become_an_array_of_the_present_parts_in_order() {
        let payload = payload(
            smallvec![json!({"a": 1}), json!(null), json!({"b": null})],
            default_of(NgsiLdAttributeKind::JsonProperty),
        );

        assert_eq!(payload, Some(json!([{"a": 1}, {"b": null}])));
    }

    #[test]
    fn several_parts_under_the_json_property_default_holding_a_non_object_are_refused() {
        assert_eq!(
            apply(smallvec![json!({"a": 1}), json!([2])], default_of(NgsiLdAttributeKind::JsonProperty), None),
            Err(AttributeRefusal::JsonPayload(JsonPayloadError::ItemNotAnObject { found: JsonShape::Array }))
        );
    }

    #[test]
    fn several_null_parts_under_the_json_property_default_are_no_value() {
        assert_eq!(
            payload(smallvec![json!(null), json!(null)], default_of(NgsiLdAttributeKind::JsonProperty)),
            None
        );
    }

    #[test]
    fn a_null_item_of_an_array_part_is_left_out_of_the_list_and_counted() {
        let (value, omitted) = omitted(smallvec![json!(["a", null, "b"])], default_of(NgsiLdAttributeKind::ListProperty));

        assert_eq!(value, Some(Value::try_from(json!(["a", "b"])).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 1,
                object_members: 0,
            }
        );
    }

    #[test]
    fn a_null_member_of_an_object_part_is_left_out_of_the_object_and_counted() {
        let (value, omitted) = omitted(smallvec![json!({"value": 21.5, "unit": null})], property(Transformation::Object));

        assert_eq!(value, Some(Value::try_from(json!({"value": 21.5})).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 0,
                object_members: 1,
            }
        );
    }

    #[test]
    fn a_null_part_is_an_absent_field_and_is_not_counted() {
        let (value, omitted) = omitted(smallvec![json!("A"), json!(null)], property(Transformation::Array));

        assert_eq!(value, Some(Value::try_from(json!(["A"])).unwrap()));
        assert!(omitted.is_empty());
    }

    #[test]
    fn an_empty_string_under_the_property_default_is_kept_as_a_value() {
        assert_eq!(
            converted(smallvec![json!("")], default_of(NgsiLdAttributeKind::Property), None),
            Some(Value::String("".into()))
        );
    }

    #[test]
    fn text_spelling_no_finite_number_under_a_float_transformation_is_refused_quoting_it() {
        assert_eq!(
            apply(smallvec![json!("NaN")], property(Transformation::Float), None),
            Err(AttributeRefusal::unusable(RequiredValue::Float, "NaN"))
        );
    }

    /// Asserts that `part` under `transformation` is refused as not the `required` scalar, quoting
    /// `quoted`.
    fn assert_scalar_refused(part: JsonValue, transformation: Transformation, required: RequiredValue, quoted: &str) {
        assert_eq!(
            apply(smallvec![part], property(transformation), None),
            Err(AttributeRefusal::unusable(required, quoted)),
            "{transformation}"
        );
    }

    /// Asserts that `part` under `transformation` resolves to no value and is not refused.
    fn assert_scalar_absent(part: JsonValue, transformation: Transformation) {
        assert_eq!(converted(smallvec![part], property(transformation), None), None, "{transformation}");
    }

    #[test]
    fn an_integer_part_and_a_whole_float_read_as_an_integer() {
        assert_eq!(
            converted(smallvec![json!("12")], property(Transformation::Integer), None),
            Some(Value::Number(Number::Integer(12)))
        );
        assert_eq!(
            converted(smallvec![json!(12.0)], property(Transformation::Integer), None),
            Some(Value::Number(Number::Integer(12)))
        );
    }

    #[test]
    fn a_number_padded_with_whitespace_reads_as_the_number_it_surrounds() {
        assert_eq!(
            converted(smallvec![json!(" 12 ")], property(Transformation::Integer), None),
            Some(Value::Number(Number::Integer(12)))
        );
        assert_eq!(
            converted(smallvec![json!("\t2,5 ")], property(Transformation::Float), None),
            Some(Value::try_from(2.5).unwrap())
        );
    }

    #[test]
    fn text_that_spells_no_integer_under_an_integer_transformation_is_refused_quoting_it() {
        assert_scalar_refused(json!("abc"), Transformation::Integer, RequiredValue::Integer, "abc");
        assert_scalar_refused(json!(" 12 apples "), Transformation::Integer, RequiredValue::Integer, "12 apples");
    }

    #[test]
    fn a_structured_value_under_an_integer_transformation_is_refused_quoting_its_rendering() {
        assert_scalar_refused(json!([1]), Transformation::Integer, RequiredValue::Integer, "[1]");
        assert_scalar_refused(json!({"n": 1}), Transformation::Integer, RequiredValue::Integer, r#"{"n":1}"#);
    }

    #[test]
    fn an_integer_out_of_the_signed_64_bit_range_is_refused_quoting_it() {
        assert_scalar_refused(
            json!("99999999999999999999"),
            Transformation::Integer,
            RequiredValue::Integer,
            "99999999999999999999",
        );
    }

    #[test]
    fn text_that_spells_no_number_under_a_float_transformation_is_refused_quoting_it() {
        assert_scalar_refused(json!("x"), Transformation::Float, RequiredValue::Float, "x");
        assert_scalar_refused(json!([1.5]), Transformation::Float, RequiredValue::Float, "[1.5]");
    }

    #[test]
    fn a_boolean_reads_its_spellings_and_a_number_as_its_truthiness() {
        for (part, expected) in [
            (json!("true"), true),
            (json!("No"), false),
            (json!(1), true),
            (json!(0.0), false),
            (json!(false), false),
        ] {
            assert_eq!(
                converted(smallvec![part.clone()], property(Transformation::Boolean), None),
                Some(Value::Boolean(expected)),
                "{part}"
            );
        }
    }

    #[test]
    fn a_value_that_spells_no_boolean_under_a_boolean_transformation_is_refused_quoting_it() {
        assert_scalar_refused(json!("maybe"), Transformation::Boolean, RequiredValue::Boolean, "maybe");
        assert_scalar_refused(json!(["true"]), Transformation::Boolean, RequiredValue::Boolean, r#"["true"]"#);
    }

    #[test]
    fn blank_text_under_a_scalar_transformation_is_absent_rather_than_refused() {
        for transformation in [Transformation::Boolean, Transformation::Integer, Transformation::Float] {
            assert_scalar_absent(json!(""), transformation);
            assert_scalar_absent(json!("   "), transformation);
        }
    }

    #[test]
    fn a_null_part_under_a_scalar_transformation_is_absent_rather_than_refused() {
        for transformation in [Transformation::Boolean, Transformation::Integer, Transformation::Float] {
            assert_scalar_absent(json!(null), transformation);
        }
    }

    #[test]
    fn several_parts_under_a_scalar_transformation_read_the_first_present_one() {
        assert_eq!(
            converted(smallvec![json!(null), json!("7"), json!("x")], property(Transformation::Integer), None),
            Some(Value::Number(Number::Integer(7)))
        );
        assert_eq!(
            apply(smallvec![json!(null), json!("x"), json!("7")], property(Transformation::Integer), None),
            Err(AttributeRefusal::unusable(RequiredValue::Integer, "x"))
        );
    }

    #[test]
    fn an_array_transformation_flattens_and_drops_nulls() {
        let value = converted(smallvec![json!([1, 2]), json!(null), json!(3)], property(Transformation::Array), None);

        assert_eq!(value, Some(Value::try_from(json!([1, 2, 3])).unwrap()));
    }

    #[test]
    fn an_empty_array_transformation_is_null() {
        let value = converted(smallvec![json!(null)], property(Transformation::Array), None);

        assert_eq!(value, None);
    }

    #[test]
    fn an_array_transformation_drops_blank_and_null_parts() {
        let value = converted(smallvec![json!(""), json!(null), json!("A")], property(Transformation::Array), None);

        assert_eq!(value, Some(Value::try_from(json!(["A"])).unwrap()));
    }

    #[test]
    fn an_array_transformation_over_only_blank_and_null_parts_is_null() {
        let value = converted(smallvec![json!(""), json!(null)], property(Transformation::Array), None);

        assert_eq!(value, None);
    }

    #[test]
    fn an_array_transformation_keeps_blank_elements_of_an_array_part() {
        let value = converted(smallvec![json!(["a", ""])], property(Transformation::Array), None);

        assert_eq!(value, Some(Value::try_from(json!(["a", ""])).unwrap()));
    }

    #[test]
    fn an_object_transformation_keeps_an_object_part() {
        let value = converted(smallvec![json!({"a": 1})], property(Transformation::Object), None);

        assert_eq!(value, Some(Value::try_from(json!({"a": 1})).unwrap()));
    }

    #[test]
    fn an_object_transformation_reads_text_written_as_an_object() {
        let value = converted(smallvec![json!(r#"{"k":1}"#)], property(Transformation::Object), None);

        assert_eq!(value, Some(Value::try_from(json!({"k": 1})).unwrap()));
    }

    #[test]
    fn a_null_member_of_object_text_is_left_out_of_the_object_and_counted() {
        let (value, omitted) = omitted(smallvec![json!(r#"{"value":21.5,"unit":null}"#)], property(Transformation::Object));

        assert_eq!(value, Some(Value::try_from(json!({"value": 21.5})).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 0,
                object_members: 1,
            }
        );
    }

    #[test]
    fn text_that_is_not_json_under_an_object_transformation_is_refused_quoting_it() {
        assert_eq!(
            apply(smallvec![json!("{not json")], property(Transformation::Object), None),
            Err(AttributeRefusal::unusable(RequiredValue::JsonObject, "{not json"))
        );
    }

    #[test]
    fn json_text_that_is_no_object_under_an_object_transformation_is_refused_quoting_it() {
        for text in ["[1,2]", "5", "\"k\"", "null"] {
            assert_eq!(
                apply(smallvec![json!(text)], property(Transformation::Object), None),
                Err(AttributeRefusal::unusable(RequiredValue::JsonObject, text)),
                "{text}"
            );
        }
    }

    #[test]
    fn blank_text_and_empty_object_text_under_an_object_transformation_are_no_value() {
        for text in ["", "  ", "{}"] {
            let value = converted(smallvec![json!(text)], property(Transformation::Object), None);

            assert_eq!(value, None, "{text:?}");
        }
    }

    #[test]
    fn a_structured_value_that_is_no_object_under_an_object_transformation_is_refused_quoting_its_rendering() {
        for (part, rendering) in [(json!([1, 2]), "[1,2]"), (json!(5), "5"), (json!(true), "true")] {
            assert_eq!(
                apply(smallvec![part], property(Transformation::Object), None),
                Err(AttributeRefusal::unusable(RequiredValue::JsonObject, rendering)),
                "{rendering}"
            );
        }
    }

    #[test]
    fn a_null_part_under_an_object_transformation_is_no_value_rather_than_a_refusal() {
        assert_eq!(converted(smallvec![json!(null)], property(Transformation::Object), None), None);
    }

    #[test]
    fn an_array_transformation_splices_in_text_written_as_an_array() {
        let value = converted(smallvec![json!("[1,2]")], property(Transformation::Array), None);

        assert_eq!(value, Some(Value::try_from(json!([1, 2])).unwrap()));
    }

    #[test]
    fn array_text_under_the_list_property_default_becomes_the_list_it_encodes() {
        let value = converted(smallvec![json!("[1,7,15]")], default_of(NgsiLdAttributeKind::ListProperty), None);

        assert_eq!(value, Some(Value::try_from(json!([1, 7, 15])).unwrap()));
    }

    #[test]
    fn text_that_is_no_json_array_under_an_array_transformation_is_one_element() {
        for text in ["BS IN", "[1,2", "5", "true", r#"{"k":1}"#] {
            let value = converted(smallvec![json!(text)], property(Transformation::Array), None);

            assert_eq!(value, Some(Value::try_from(json!([text])).unwrap()), "{text}");
        }
    }

    #[test]
    fn a_null_item_of_array_text_is_left_out_of_the_list_and_counted() {
        let (value, omitted) = omitted(smallvec![json!(r#"["a",null,"b"]"#)], default_of(NgsiLdAttributeKind::ListProperty));

        assert_eq!(value, Some(Value::try_from(json!(["a", "b"])).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 1,
                object_members: 0,
            }
        );
    }

    #[test]
    fn array_text_and_other_parts_are_flattened_in_order() {
        let value = converted(smallvec![json!("[1,2]"), json!("BS"), json!([3])], property(Transformation::Array), None);

        assert_eq!(value, Some(Value::try_from(json!([1, 2, "BS", 3])).unwrap()));
    }

    #[test]
    fn empty_array_text_under_an_array_transformation_is_no_value() {
        let value = converted(smallvec![json!("[]")], property(Transformation::Array), None);

        assert_eq!(value, None);
    }

    #[test]
    fn a_json_transformation_reads_text_as_the_json_value_it_is_written_as() {
        for (text, expected) in [
            (r#"{"no2":3,"pm10":2}"#, json!({"no2": 3, "pm10": 2})),
            ("[1,7,15]", json!([1, 7, 15])),
            ("3.5", json!(3.5)),
            ("true", json!(true)),
            (r#""hello""#, json!("hello")),
        ] {
            let value = converted(smallvec![json!(text)], property(Transformation::Json), None);

            assert_eq!(value, Some(Value::try_from(expected).unwrap()), "{text}");
        }
    }

    #[test]
    fn a_value_that_is_not_text_under_a_json_transformation_is_kept_as_it_is() {
        for part in [json!(7), json!(false), json!([1, "a"]), json!({"k": 1})] {
            // The part is consumed by the conversion and still needed to build the expected value.
            let value = converted(smallvec![part.clone()], property(Transformation::Json), None);

            assert_eq!(value, Some(Value::try_from(part).unwrap()));
        }
    }

    #[test]
    fn text_that_is_not_json_under_a_json_transformation_is_refused_quoting_it() {
        assert_eq!(
            apply(smallvec![json!("hello")], property(Transformation::Json), None),
            Err(AttributeRefusal::unusable(RequiredValue::Json, "hello"))
        );
    }

    #[test]
    fn blank_text_and_null_text_under_a_json_transformation_are_no_value() {
        for part in [json!(""), json!("  "), json!("null"), json!(null)] {
            let value = converted(smallvec![part], property(Transformation::Json), None);

            assert_eq!(value, None, "{value:?}");
        }
    }

    #[test]
    fn a_null_nested_in_json_text_under_a_json_transformation_is_left_out_and_counted() {
        let (value, omitted) = omitted(smallvec![json!("[1,null]")], property(Transformation::Json));

        assert_eq!(value, Some(Value::try_from(json!([1])).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 1,
                object_members: 0,
            }
        );
    }

    #[test]
    fn text_under_the_json_property_default_is_not_decoded() {
        assert_eq!(
            apply(smallvec![json!(r#"{"k":1}"#)], default_of(NgsiLdAttributeKind::JsonProperty), None),
            Err(AttributeRefusal::JsonPayload(JsonPayloadError::NotAnObject { found: JsonShape::String }))
        );
    }

    #[test]
    fn a_decoded_conversion_reads_object_text_as_raw_json_keeping_its_nulls() {
        let payload = payload(smallvec![json!(r#"{"no2":3,"pm10":null}"#)], Conversion::JsonProperty(JsonConversion::Json));

        assert_eq!(payload, Some(json!({"no2": 3, "pm10": null})));
    }

    #[test]
    fn a_decoded_conversion_reads_text_of_an_array_of_objects() {
        let payload = payload(smallvec![json!(r#"[{"a":1},{"b":2}]"#)], Conversion::JsonProperty(JsonConversion::Json));

        assert_eq!(payload, Some(json!([{"a": 1}, {"b": 2}])));
    }

    #[test]
    fn a_decoded_conversion_keeps_a_structured_part_as_it_is() {
        let payload = payload(smallvec![json!({"maybe": null})], Conversion::JsonProperty(JsonConversion::Json));

        assert_eq!(payload, Some(json!({"maybe": null})));
    }

    #[test]
    fn text_that_is_not_json_under_a_decoded_conversion_is_refused_quoting_it() {
        assert_eq!(
            apply(smallvec![json!("{not json")], Conversion::JsonProperty(JsonConversion::Json), None),
            Err(AttributeRefusal::unusable(RequiredValue::Json, "{not json"))
        );
    }

    #[test]
    fn scalar_text_under_a_decoded_conversion_is_no_json_property_payload() {
        assert_eq!(
            apply(smallvec![json!("5")], Conversion::JsonProperty(JsonConversion::Json), None),
            Err(AttributeRefusal::JsonPayload(JsonPayloadError::NotAnObject { found: JsonShape::Number }))
        );
    }

    #[test]
    fn blank_text_under_a_decoded_conversion_is_no_value() {
        assert_eq!(payload(smallvec![json!("  ")], Conversion::JsonProperty(JsonConversion::Json)), None);
    }

    #[test]
    fn several_parts_under_a_decoded_conversion_become_an_array_of_the_present_decoded_parts() {
        let payload = payload(
            smallvec![json!(r#"{"a":1}"#), json!(""), json!({"b": 2})],
            Conversion::JsonProperty(JsonConversion::Json),
        );

        assert_eq!(payload, Some(json!([{"a": 1}, {"b": 2}])));
    }

    #[test]
    fn an_object_transformation_over_an_empty_object_is_null() {
        let value = converted(smallvec![json!({})], property(Transformation::Object), None);

        assert_eq!(value, None);
    }

    #[test]
    fn a_numeric_epoch_reads_as_the_same_instant_as_its_text() {
        for transformation in [Transformation::DateTime, Transformation::Date, Transformation::Time] {
            let number = converted(smallvec![json!(1_775_253_620)], property(transformation), None);
            let text = converted(smallvec![json!("1775253620")], property(transformation), None);

            assert!(matches!(number, Some(Value::Temporal(_))), "{transformation:?} left {number:?}");
            assert_eq!(number, text);
        }
    }

    #[test]
    fn a_fractional_numeric_epoch_reads_as_an_instant() {
        let value = converted(smallvec![json!(1_775_253_620.5)], property(Transformation::DateTime), None);

        assert!(matches!(value, Some(Value::Temporal(_))));
    }

    #[test]
    fn a_number_too_small_for_an_epoch_is_refused_quoting_it() {
        let refusal = apply(smallvec![json!(2026)], property(Transformation::DateTime), None);

        assert!(matches!(refusal, Err(AttributeRefusal::UnreadableTimestamp { ref text }) if text.as_ref() == "2026"));
    }

    #[test]
    fn a_boolean_under_a_temporal_transformation_is_refused_quoting_it() {
        let refusal = apply(smallvec![json!(true)], property(Transformation::DateTime), None);

        assert!(matches!(refusal, Err(AttributeRefusal::UnreadableTimestamp { ref text }) if text.as_ref() == "true"));
    }

    #[test]
    fn a_blank_temporal_source_is_still_absent_rather_than_refused() {
        let value = converted(smallvec![json!("  ")], property(Transformation::DateTime), None);

        assert_eq!(value, None);
    }

    #[test]
    fn a_point_transformation_builds_a_geospatial_value() {
        let value = converted(smallvec![json!(14.5), json!(46.0)], property(Transformation::Point), None);

        assert!(matches!(value, Some(Value::Geospatial(_))));
    }

    #[test]
    fn a_geometry_transformation_keeps_an_already_formed_geometry() {
        let source = json!({"type": "Point", "coordinates": [9.17, 45.47]});
        let value = converted(smallvec![source], property(Transformation::Geometry), None);

        assert!(matches!(value, Some(Value::Geospatial(_))));
    }

    #[test]
    fn a_geometry_transformation_on_text_that_is_no_geometry_is_refused_quoting_the_text() {
        assert_eq!(
            apply(smallvec![json!("not a geometry")], property(Transformation::Geometry), None),
            Err(AttributeRefusal::unusable(RequiredValue::Geometry, "not a geometry"))
        );
    }

    #[test]
    fn a_geojson_object_under_the_geo_property_default_is_kept_as_its_geometry() {
        let source = json!({"type": "Point", "coordinates": [14.5, 46.05]});
        let value = converted(smallvec![source], default_of(NgsiLdAttributeKind::GeoProperty), None);

        assert_eq!(
            value,
            Some(Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [14.5, 46.05].into(),
            })))
        );
    }

    #[test]
    fn geojson_text_under_the_geo_property_default_is_read_as_its_geometry() {
        let source = json!(r#"{"type":"Point","coordinates":[14.5,46.05]}"#);
        let value = converted(smallvec![source], default_of(NgsiLdAttributeKind::GeoProperty), None);

        assert_eq!(
            value,
            Some(Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [14.5, 46.05].into(),
            })))
        );
    }

    #[test]
    fn a_null_or_blank_part_under_the_geo_property_default_is_no_value_rather_than_a_refusal() {
        for part in [json!(""), json!("  "), json!(null)] {
            let value = converted(smallvec![part], default_of(NgsiLdAttributeKind::GeoProperty), None);

            assert_eq!(value, None, "{value:?}");
        }
    }

    #[test]
    fn blank_or_null_coordinate_parts_under_a_point_transformation_are_no_value_rather_than_a_refusal() {
        let value = converted(smallvec![json!(""), json!(null)], property(Transformation::Point), None);

        assert_eq!(value, None);
    }

    #[test]
    fn a_value_spelling_no_geometry_under_the_geo_property_default_is_refused_quoting_it() {
        for (part, quoted) in [
            (json!("not geojson"), "not geojson"),
            (json!({"type": "Point"}), r#"{"type":"Point"}"#),
            (json!(42), "42"),
            (json!(true), "true"),
        ] {
            assert_eq!(
                apply(smallvec![part], default_of(NgsiLdAttributeKind::GeoProperty), None),
                Err(AttributeRefusal::unusable(RequiredValue::Geometry, quoted)),
                "{quoted}"
            );
        }
    }

    #[test]
    fn a_coordinate_part_left_blank_beside_one_present_under_a_point_transformation_is_refused_quoting_both() {
        assert_eq!(
            apply(smallvec![json!(""), json!("46.0")], property(Transformation::Point), None),
            Err(AttributeRefusal::unusable(RequiredValue::Geometry, r#"["","46.0"]"#))
        );
    }

    #[test]
    fn the_geo_property_default_applies_the_declared_geometry_policy() {
        let source = json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]],
        });
        let policy = GeometryPolicy::builder().convert(Some(ConversionStrategy::Centroid)).build();
        let value = converted(smallvec![source], default_of(NgsiLdAttributeKind::GeoProperty), Some(&policy));

        assert_eq!(
            value,
            Some(Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [1.0, 1.0].into(),
            })))
        );
    }

    #[test]
    fn a_source_geometry_collection_under_the_geo_property_default_is_refused() {
        let source = json!({"type": "GeometryCollection", "geometries": []});

        assert_eq!(
            apply(smallvec![source], default_of(NgsiLdAttributeKind::GeoProperty), None),
            Err(AttributeRefusal::Geometry(GeometryError::GeometryCollection))
        );
    }

    #[test]
    fn an_explicit_string_transformation_still_turns_a_geometry_into_its_compact_json_text() {
        let source = json!({"type": "Point", "coordinates": [14.5, 46.05]});
        let value = converted(smallvec![source], property(Transformation::String), None);

        assert_eq!(value, Some(Value::String(r#"{"type":"Point","coordinates":[14.5,46.05]}"#.into())));
    }

    #[test]
    fn a_point_source_promotes_to_a_declared_multipoint() {
        let source = json!({"type": "Point", "coordinates": [9.17, 45.47]});
        let value = converted(smallvec![source], property(Transformation::MultiPoint), None);

        let Some(Value::Geospatial(geometry)) = value else {
            panic!("a geometry");
        };
        assert_eq!(geometry.kind(), GeometryKind::MultiPoint);
    }

    #[test]
    fn a_lossy_geometry_conversion_is_refused_rather_than_silently_dropped() {
        let source = json!({
            "type": "MultiPolygon",
            "coordinates": [
                [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                [[[5.0, 5.0], [6.0, 5.0], [6.0, 6.0], [5.0, 5.0]]],
            ],
        });

        assert_eq!(
            apply(smallvec![source], property(Transformation::Polygon), None),
            Err(AttributeRefusal::Geometry(GeometryError::AmbiguousMultiGeometry {
                origin: GeometryKind::MultiPolygon,
                members: 2,
            }))
        );
    }

    #[test]
    fn a_declared_conversion_makes_the_same_source_convert() {
        let source = json!({
            "type": "MultiPolygon",
            "coordinates": [
                [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                [[[5.0, 5.0], [8.0, 5.0], [8.0, 8.0], [5.0, 5.0]]],
            ],
        });
        let policy = GeometryPolicy::builder().convert(Some(ConversionStrategy::Largest)).build();
        let value = converted(smallvec![source], property(Transformation::Polygon), Some(&policy));

        let Some(Value::Geospatial(geometry)) = value else {
            panic!("a geometry");
        };
        assert_eq!(geometry.kind(), GeometryKind::Polygon);
    }

    #[test]
    fn a_source_geometry_collection_is_refused() {
        let source = json!({"type": "GeometryCollection", "geometries": []});

        assert_eq!(
            apply(smallvec![source], property(Transformation::Geometry), None),
            Err(AttributeRefusal::Geometry(GeometryError::GeometryCollection))
        );
    }

    #[test]
    fn a_clockwise_exterior_ring_is_emitted_counterclockwise() {
        let source = json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]],
        });
        let value = converted(smallvec![source], property(Transformation::Polygon), None);

        let Some(Value::Geospatial(geometry)) = value else {
            panic!("a geometry");
        };
        assert_eq!(
            *geometry,
            NgsiLdGeometry::Polygon {
                coordinates: vec![vec![
                    [0.0, 0.0].into(),
                    [1.0, 0.0].into(),
                    [1.0, 1.0].into(),
                    [0.0, 1.0].into(),
                    [0.0, 0.0].into(),
                ]],
            }
        );
    }
}
