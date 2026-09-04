use tera::{Error, Kwargs, TeraResult, Value};

/// Wraps a computed number as a Tera value, collapsing every non-finite result to a null.
///
/// The mapping layer treats an attribute whose value resolves to null as simply absent: the
/// attribute is dropped and the surrounding entity survives. Math over source data routinely
/// produces a domain-invalid result (`sqrt` of a negative, `ln` of zero, `asin` outside `[-1, 1]`,
/// a `map_range` whose input span is zero), and the right outcome for one bad cell is to omit that
/// one attribute, never to error and lose the whole record. So a `NaN` or an infinity becomes a
/// null here rather than being handed back to Tera as a number.
pub(crate) fn finite_or_none(value: f64) -> Value {
    if value.is_finite() { Value::from(value) } else { Value::none() }
}

/// Reads a numeric value that a mapping may have written either as a number or as the numeric
/// string that text-shaped sources (CSV columns, XML text nodes) carry.
///
/// `label` names the value in any error message: a filter name for a piped value, an argument name
/// for a keyword argument.
///
/// # Errors
/// Returns a [`tera::Error`] when the value is neither a number nor a string parseable as one. That
/// is a mapping misconfiguration (a non-numeric field fed to arithmetic), so failing the record is
/// the correct signal; a merely out-of-domain number is handled by [`finite_or_none`] instead.
pub(crate) fn number_from_value(value: &Value, label: &str) -> TeraResult<f64> {
    if let Some(number) = value.as_f64() {
        return Ok(number);
    }

    if let Some(text) = value.as_str() {
        return text
            .trim()
            .parse()
            .map_err(|_| Error::message(format!("`{label}` value `{text}` is not a number")));
    }

    Err(Error::message(format!("`{label}` must be a number or a numeric string, got {value}")))
}

/// Reads a required keyword argument as a number, tolerating the numeric-string form.
///
/// # Errors
/// Returns a [`tera::Error`] when the argument is absent, or present but not readable as a number
/// (see [`number_from_value`]).
pub(crate) fn number_arg(kwargs: &Kwargs, name: &str) -> TeraResult<f64> {
    let Some(argument) = kwargs.get::<Value>(name)? else {
        return Err(Error::message(format!("argument `{name}` is required")));
    };

    number_from_value(&argument, name)
}

#[cfg(test)]
mod tests {
    use crate::template::numeric_value::{finite_or_none, number_from_value};
    use tera::Value;

    #[test]
    fn a_finite_result_is_kept_as_a_number() {
        assert_eq!(finite_or_none(2.5).as_f64(), Some(2.5));
    }

    #[test]
    fn a_nan_result_becomes_a_null() {
        assert!(finite_or_none(f64::NAN).is_none());
    }

    #[test]
    fn an_infinite_result_becomes_a_null() {
        assert!(finite_or_none(f64::INFINITY).is_none());
    }

    #[test]
    fn a_numeric_value_reads_directly() {
        assert!((number_from_value(&Value::from(3.0), "x").unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_numeric_string_value_is_parsed() {
        assert!((number_from_value(&Value::from(" 3.5 "), "x").unwrap() - 3.5).abs() < 1e-9);
    }

    #[test]
    fn a_non_numeric_string_value_is_an_error() {
        assert!(number_from_value(&Value::from("north"), "x").is_err());
    }

    #[test]
    fn a_non_numeric_value_is_an_error() {
        assert!(number_from_value(&Value::from(true), "x").is_err());
    }
}
