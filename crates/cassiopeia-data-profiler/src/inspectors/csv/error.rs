use std::io;

/// Errors that can occur while inspecting CSV input.
#[derive(Debug, thiserror::Error)]
pub enum CsvInspectError {
    /// The input sample was empty, so no dialect could be inspected.
    #[error("the CSV input is empty")]
    EmptyInput,

    /// No delimiter/quote/header combination produced a coherent table.
    #[error("no valid CSV dialect could be detected")]
    NoDialectDetected,

    /// Reading the sample used for dialect inspection failed.
    #[error("failed to read the CSV sample")]
    SampleRead(#[source] io::Error),
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::error::CsvInspectError;
    use std::{error::Error, io};

    #[test]
    fn a_sample_read_failure_exposes_the_operating_system_reason_as_its_cause() {
        let error = CsvInspectError::SampleRead(io::Error::other("input/output error"));

        assert_eq!(error.source().expect("a chained cause").to_string(), "input/output error");
    }
}
