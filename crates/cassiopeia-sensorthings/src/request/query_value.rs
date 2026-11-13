use percent_encoding::{AsciiSet, CONTROLS, PercentEncode, utf8_percent_encode};

/// The bytes a query value is percent-encoded in: those a URL query cannot hold literally, those
/// that separate query parameters or stand for a space, and `'`, which the WHATWG URL Standard's
/// special-query percent-encode set encodes in every `http` and `https` URL. Encoding it here keeps
/// the request Cassiopeia logs identical to the one it sends. Every other ASCII byte, `OData`'s
/// `$ ( ) ; , = / :` among them, is kept, and every non-ASCII byte is encoded.
const QUERY_VALUE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'\'')
    .add(b'+')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// `text` written as the value of a query parameter: percent-encoded where a URL query cannot hold a
/// byte literally, where a byte would separate parameters or stand for a space, and at `'`, and
/// unchanged elsewhere, `OData`'s punctuation among it.
#[must_use]
pub fn query_value(text: &str) -> PercentEncode<'_> {
    utf8_percent_encode(text, QUERY_VALUE)
}

#[cfg(test)]
mod tests {
    use crate::request::query_value::query_value;
    use pretty_assertions::assert_eq;
    use url::Url;

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(
            query_value("ObservedProperty/name eq 'pm10'").to_string(),
            "ObservedProperty/name%20eq%20%27pm10%27"
        );
        assert_eq!(query_value("a+b&c#d").to_string(), "a%2Bb%26c%23d");
        assert_eq!(query_value("µ").to_string(), "%C2%B5");
    }

    #[test]
    fn odata_punctuation_is_kept() {
        assert_eq!(
            query_value("endswith(name,'-pm10');$top=1/x:y?@!*~-._").to_string(),
            "endswith(name,%27-pm10%27);$top=1/x:y?@!*~-._"
        );
    }

    #[test]
    fn an_encoded_value_survives_url_parsing_unchanged() {
        let value = query_value("Thing/properties/name eq 'a \"b\" <c> {d} [e] |f| ^g^ `h` \\i'").to_string();

        let url = Url::parse(&format!("https://h.example/v1.1/Datastreams?$filter={value}")).unwrap();

        assert_eq!(url.query(), Some(format!("$filter={value}").as_str()));
    }
}
