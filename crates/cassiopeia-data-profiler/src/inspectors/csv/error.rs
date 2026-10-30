/// Errors that can occur while inspecting CSV input.
#[derive(Debug, thiserror::Error)]
pub enum CsvInspectError {
    /// The content was empty, so no dialect could be inspected.
    #[error("the CSV input is empty")]
    EmptyInput,

    /// No delimiter/quote/header combination produced a coherent table.
    #[error("no valid CSV dialect could be detected")]
    NoDialectDetected,
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::error::CsvInspectError;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_empty_input_is_reported_as_an_empty_csv() {
        assert_eq!(CsvInspectError::EmptyInput.to_string(), "the CSV input is empty");
    }
}
