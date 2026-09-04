use crate::transformation::Transformation;
use cassiopeia_geometry::{strategy::ConversionStrategy, target::GeometryTarget};
use geojson::Geometry;
use serde_json::Value as JsonValue;
use tera::{Error, Kwargs, TeraResult, Value};

/// What the `value` argument of a `geo_*` function carried.
pub enum GeometryArgument {
    /// Nothing: the argument was absent or null, so there is no geometry to work on and the
    /// attribute simply drops, the same way a missing source field does.
    Absent,
    /// A `GeoJSON` geometry object (RFC 7946 clause 3.1), still unadmitted: a `GeometryCollection`
    /// reaches the caller so a declared `flatten` can still fold it.
    Present(Geometry),
}

/// Reads the `value` argument of a `geo_*` function as a `GeoJSON` geometry.
///
/// The argument may be the geometry object itself (as a source that ingested `GeoJSON`, KML or a
/// shapefile carries it) or the JSON text of one, as a CSV cell or an XML text node carries it.
///
/// # Errors
/// Returns a [`tera::Error`] when the argument is present but is not a `GeoJSON` geometry. That is a
/// mapping misconfiguration rather than bad data in one cell, so failing the record is the right
/// signal.
pub fn read_geometry(kwargs: &Kwargs, function: &str) -> TeraResult<GeometryArgument> {
    let Some(argument) = kwargs.get::<Value>("value")? else {
        return Ok(GeometryArgument::Absent);
    };
    if argument.is_none() || argument.is_undefined() {
        return Ok(GeometryArgument::Absent);
    }

    let json = serde_json::to_value(&argument).map_err(|error| Error::message(format!("Function `{function}` could not read `value`: {error}")))?;
    let geometry = match &json {
        JsonValue::String(text) if text.trim().is_empty() => return Ok(GeometryArgument::Absent),
        JsonValue::String(text) => serde_json::from_str::<Geometry>(text).ok(),
        JsonValue::Null => return Ok(GeometryArgument::Absent),
        JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_) => serde_json::from_value::<Geometry>(json.clone()).ok(),
    };

    geometry
        .map(GeometryArgument::Present)
        .ok_or_else(|| Error::message(format!("Function `{function}` needs a GeoJSON geometry for `value`, got {argument}")))
}

/// Reads the required `to` argument as the geometry type to produce.
///
/// The vocabulary is the mapping's own `transformation` tokens (`point`, `multipolygon`,
/// `geometry`), so a template and an attribute declaration name a target the same way.
///
/// # Errors
/// Returns a [`tera::Error`] when the argument is absent, is not a string, or does not name a
/// geometry type.
pub fn read_target(kwargs: &Kwargs, function: &str) -> TeraResult<GeometryTarget> {
    let Some(argument) = kwargs.get::<Value>("to")? else {
        return Err(Error::message(format!("Function `{function}` needs a `to` argument")));
    };
    let Some(token) = argument.as_str() else {
        return Err(Error::message(format!("Function `{function}` needs a string for `to`, got {argument}")));
    };

    serde_json::from_value::<Transformation>(JsonValue::String(token.to_string()))
        .ok()
        .and_then(Transformation::geometry_target)
        .ok_or_else(|| Error::message(format!("Function `{function}` does not know the geometry type `{token}`")))
}

/// Reads the optional `using` argument as the conversion to apply.
///
/// # Errors
/// Returns a [`tera::Error`] when the argument is present but does not name a conversion.
pub fn read_strategy(kwargs: &Kwargs, function: &str) -> TeraResult<Option<ConversionStrategy>> {
    let Some(argument) = kwargs.get::<Value>("using")? else {
        return Ok(None);
    };
    if argument.is_none() || argument.is_undefined() {
        return Ok(None);
    }
    let Some(token) = argument.as_str() else {
        return Err(Error::message(format!("Function `{function}` needs a string for `using`, got {argument}")));
    };

    serde_json::from_value::<ConversionStrategy>(JsonValue::String(token.to_string()))
        .map(Some)
        .map_err(|_error| Error::message(format!("Function `{function}` does not know the conversion `{token}`")))
}
