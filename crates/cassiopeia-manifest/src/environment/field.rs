use derive_more::Display;
use http::HeaderName;

/// A manifest field that may read environment variables, named the way an error reports it.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum TemplatedField {
    /// An input's `source`.
    #[display("an input's 'source'")]
    InputSource,
    /// The output's broker `url`.
    #[display("the output 'url'")]
    OutputUrl,
    /// One of an input's `headers`.
    #[display("the input header '{_0}'")]
    InputHeader(HeaderName),
    /// One of the output's `headers`.
    #[display("the output header '{_0}'")]
    OutputHeader(HeaderName),
}

#[cfg(test)]
mod tests {
    use crate::environment::field::TemplatedField;
    use http::HeaderName;
    use pretty_assertions::assert_eq;

    #[test]
    fn each_field_names_where_it_sits() {
        assert_eq!(TemplatedField::InputSource.to_string(), "an input's 'source'");
        assert_eq!(TemplatedField::OutputUrl.to_string(), "the output 'url'");
        assert_eq!(
            TemplatedField::InputHeader(HeaderName::from_static("x-api-key")).to_string(),
            "the input header 'x-api-key'"
        );
        assert_eq!(
            TemplatedField::OutputHeader(HeaderName::from_static("authorization")).to_string(),
            "the output header 'authorization'"
        );
    }
}
