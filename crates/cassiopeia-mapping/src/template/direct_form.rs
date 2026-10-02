use crate::template::{CompiledTemplate, TemplatePart, field_path::FieldPath, identifier::is_identifier};
use lazy_regex::regex;

/// Head identifiers a bare reference may not start with, because Tera does not read them as a
/// record field.
///
/// Tera's lexer turns `true`, `True`, `false`, and `False` into booleans, its parser turns `none`,
/// `None`, and `null` into the empty value and `not` into negation, and its interpreter answers
/// `__tera_context` with a dump of the whole context. `this` is bound to the record itself by the
/// resolver, not to a field named `this`.
const ENGINE_HEADS: [&str; 10] = ["true", "True", "false", "False", "none", "None", "null", "not", "__tera_context", "this"];

/// Path segments after a dot that Tera's lexer reads as booleans rather than as an attribute name.
const BOOLEAN_SEGMENTS: [&str; 4] = ["true", "True", "false", "False"];

/// Compiles a template that resolves without the templating engine, or returns `None` when it needs
/// Tera.
///
/// A template resolves directly when it holds no `{%` tag and no `{#` comment, and every `{{` is
/// closed by a `}}` around exactly one field reference (see [`field_reference`]). Text outside the
/// delimiters is literal, exactly as Tera emits it. Anything else, malformed templates included, is
/// left to Tera, which either evaluates it or reports why it cannot. This is the one definition of
/// the direct forms; classifying a template and splitting it are the same pass, so they cannot
/// disagree.
pub(crate) fn compile(source: &str) -> Option<CompiledTemplate> {
    if source.contains("{%") || source.contains("{#") {
        return None;
    }

    let mut parts = Vec::new();
    let mut rest = source;
    while let Some((literal, opened)) = rest.split_once("{{") {
        let (body, closed) = opened.split_once("}}")?;
        if !literal.is_empty() {
            parts.push(TemplatePart::Static(literal.to_string()));
        }
        parts.push(TemplatePart::Dynamic(field_reference(body)?));
        rest = closed;
    }
    if !rest.is_empty() {
        parts.push(TemplatePart::Static(rest.to_string()));
    }

    Some(match <[TemplatePart; 1]>::try_from(parts) {
        Ok([TemplatePart::Static(literal)]) => CompiledTemplate::Static(literal),
        Ok([TemplatePart::Dynamic(key)]) => CompiledTemplate::Simple(key),
        Err(parts) if parts.is_empty() => CompiledTemplate::Static(String::new()),
        Err(parts) => CompiledTemplate::Composite(parts),
    })
}

/// Reads the body of one `{{ }}` as a field reference, or returns `None` when it is any other
/// expression.
///
/// The grammar is the subset of Tera's expression syntax whose meaning a [`FieldPath`] reproduces
/// exactly, surrounded by the ASCII whitespace Tera skips inside delimiters:
///
/// - a dotted path of identifiers (see [`identifier`](crate::template::identifier)), `name`,
///   `properties.name`, or `ulica.številka`, whose head is not one of [`ENGINE_HEADS`] and whose
///   later segments are not one of [`BOOLEAN_SEGMENTS`]. A path with a non-ASCII segment is read
///   here because Tera cannot read it at all: its lexer accepts only ASCII identifiers;
/// - `this['key']`, a single-quoted key with no `.` (a path would split it into segments where Tera
///   reads one key) and no `\` (Tera unescapes it);
/// - `this[n]`, a decimal column index that Tera's lexer accepts as an integer, named by its value.
///
/// A `-` whitespace-control marker, an operator, a filter, a call, or a literal makes the body
/// something else, as does whitespace between the tokens of a path.
fn field_reference(body: &str) -> Option<FieldPath> {
    let reference = body.trim_matches(|character: char| character.is_ascii_whitespace());

    if let Some(captures) = regex!(r"^this\[(?:'([^'.\\]*)'|([0-9]+))\]$").captures(reference) {
        if let Some(key) = captures.get(1) {
            return Some(FieldPath::new(key.as_str()));
        }
        // Tera reads the digits as an `i64`, so `this[007]` indexes column 7 and an index past the
        // `i64` range does not parse at all.
        let index: i64 = captures.get(2)?.as_str().parse().ok()?;
        return Some(FieldPath::new(index.to_string()));
    }

    let mut segments = reference.split('.');
    let head = segments.next()?;
    let is_path =
        is_identifier(head) && !ENGINE_HEADS.contains(&head) && segments.all(|segment| is_identifier(segment) && !BOOLEAN_SEGMENTS.contains(&segment));

    is_path.then(|| FieldPath::new(reference))
}

#[cfg(test)]
mod tests {
    use crate::template::{
        CompiledTemplate,
        TemplatePart,
        direct_form::{compile, field_reference},
    };

    /// The field path a body reads, as text, or `None` when it is not a field reference.
    fn reference(body: &str) -> Option<String> {
        field_reference(body).map(|path| path.to_string())
    }

    #[test]
    fn a_template_without_delimiters_is_a_literal() {
        assert!(matches!(compile("Start }} %} #}"), Some(CompiledTemplate::Static(literal)) if literal == "Start }} %} #}"));
        assert!(matches!(compile(""), Some(CompiledTemplate::Static(literal)) if literal.is_empty()));
    }

    #[test]
    fn a_lone_reference_is_a_direct_lookup() {
        assert!(matches!(compile("{{ id }}"), Some(CompiledTemplate::Simple(key)) if key.as_str() == "id"));
    }

    #[test]
    fn literal_text_around_references_is_a_concatenation() {
        let Some(CompiledTemplate::Composite(parts)) = compile("a{{ x }}b{{ y }}") else {
            panic!("expected a concatenation");
        };

        assert!(matches!(
            parts.as_slice(),
            [TemplatePart::Static(a), TemplatePart::Dynamic(x), TemplatePart::Static(b), TemplatePart::Dynamic(y)]
                if a == "a" && x.as_str() == "x" && b == "b" && y.as_str() == "y"
        ));
    }

    #[test]
    fn two_adjacent_references_are_a_concatenation() {
        assert!(matches!(compile("{{ a }}{{ b }}"), Some(CompiledTemplate::Composite(parts)) if parts.len() == 2));
    }

    #[test]
    fn a_tag_or_a_comment_anywhere_needs_the_engine() {
        assert!(compile("{% if a %}{{ a }}{% endif %}").is_none());
        assert!(compile("{%if a%}x{%endif%}").is_none());
        assert!(compile("{{ a }} {# note #}").is_none());
        assert!(compile("text {% raw %}").is_none());
    }

    #[test]
    fn an_unclosed_reference_needs_the_engine() {
        assert!(compile("Station-{{ id").is_none());
        assert!(compile("{{ a }} {{ b").is_none());
    }

    #[test]
    fn a_body_that_is_not_a_reference_needs_the_engine() {
        assert!(compile("Station-{{ id | upper }}").is_none());
        assert!(compile("{{ a + 1 }}").is_none());
    }

    #[test]
    fn a_dotted_path_of_identifiers_is_a_reference() {
        assert_eq!(reference("a").as_deref(), Some("a"));
        assert_eq!(reference(" properties.name ").as_deref(), Some("properties.name"));
        assert_eq!(reference("_x9.y_.Z").as_deref(), Some("_x9.y_.Z"));
        assert_eq!(reference("vars.valid_from").as_deref(), Some("vars.valid_from"));
    }

    #[test]
    fn surrounding_ascii_whitespace_is_ignored() {
        assert_eq!(reference("\t\n\x0C\r id \n").as_deref(), Some("id"));
    }

    #[test]
    fn whitespace_tera_does_not_skip_is_not_ignored() {
        assert_eq!(reference("\u{a0}id"), None);
        assert_eq!(reference("id\u{0B}"), None);
    }

    #[test]
    fn a_quoted_key_on_this_is_a_reference_to_that_key() {
        assert_eq!(reference("this['CO(GT)']").as_deref(), Some("CO(GT)"));
        assert_eq!(reference("this['City Name']").as_deref(), Some("City Name"));
        assert_eq!(reference("this['']").as_deref(), Some(""));
    }

    #[test]
    fn a_quoted_key_a_path_would_misread_is_not_a_reference() {
        assert_eq!(reference("this['a.b']"), None);
        assert_eq!(reference(r"this['a\'b']"), None);
        assert_eq!(reference(r#"this["a"]"#), None);
    }

    #[test]
    fn a_positional_index_on_this_names_its_column_by_value() {
        assert_eq!(reference("this[0]").as_deref(), Some("0"));
        assert_eq!(reference("this[12]").as_deref(), Some("12"));
        assert_eq!(reference("this[007]").as_deref(), Some("7"));
    }

    #[test]
    fn a_positional_index_tera_cannot_lex_is_not_a_reference() {
        assert_eq!(reference("this[99999999999999999999]"), None);
    }

    #[test]
    fn names_tera_reads_as_something_other_than_a_field_are_not_references() {
        for body in [
            "true",
            "True",
            "false",
            "False",
            "none",
            "None",
            "null",
            "not",
            "__tera_context",
            "this",
            "this.a",
        ] {
            assert_eq!(reference(body), None, "{body}");
        }
    }

    #[test]
    fn a_boolean_after_a_dot_is_not_a_reference() {
        assert_eq!(reference("a.true"), None);
        assert_eq!(reference("a.False"), None);
    }

    #[test]
    fn reserved_words_tera_reads_as_variables_are_references() {
        for body in ["and", "or", "in", "is", "loop", "self", "a.not", "a.none", "context"] {
            assert_eq!(reference(body).as_deref(), Some(body), "{body}");
        }
    }

    #[test]
    fn any_other_expression_is_not_a_reference() {
        for body in [
            "-a",
            "a -",
            "a.0",
            "a . b",
            "a..b",
            ".a",
            "a.",
            "1a",
            "a b",
            "a | upper",
            "a(b)",
            "a[0]",
            "a['b']",
            "this [0]",
            "this[ 0 ]",
            "this[-1]",
            "'lit'",
            "42",
            "a ~ b",
            "station-id",
            "",
        ] {
            assert_eq!(reference(body), None, "{body}");
        }
    }

    #[test]
    fn a_lone_non_ascii_reference_is_a_direct_lookup() {
        assert!(matches!(compile("{{ čas }}"), Some(CompiledTemplate::Simple(key)) if key.as_str() == "čas"));
    }

    #[test]
    fn literal_text_around_a_non_ascii_reference_is_a_concatenation() {
        let Some(CompiledTemplate::Composite(parts)) = compile("Ura-{{ čas }}") else {
            panic!("expected a concatenation");
        };

        assert!(matches!(
            parts.as_slice(),
            [TemplatePart::Static(prefix), TemplatePart::Dynamic(key)] if prefix == "Ura-" && key.as_str() == "čas"
        ));
    }

    #[test]
    fn a_dotted_non_ascii_path_is_a_direct_lookup_of_both_segments() {
        let Some(CompiledTemplate::Simple(path)) = compile("{{ ulica.številka }}") else {
            panic!("expected a direct lookup");
        };

        assert_eq!(path.as_str(), "ulica.številka");
        assert_eq!(path.read(&serde_json::json!({"ulica": {"številka": 12}})), serde_json::json!(12));
    }

    #[test]
    fn unicode_identifiers_are_references() {
        for body in ["Città", "naïve", "_x", "číslo_2", "a.naïve.ž"] {
            assert_eq!(reference(body).as_deref(), Some(body), "{body}");
        }
    }

    #[test]
    fn non_ascii_text_that_is_not_an_identifier_is_not_a_reference() {
        for body in ["č as", "č-a", "a²", "čas.", ".čas", "čas.true", "\u{a0}čas"] {
            assert_eq!(reference(body), None, "{body}");
        }
        assert!(compile("{{ č as }}").is_none());
        assert!(compile("{{ č-a }}").is_none());
    }
}
