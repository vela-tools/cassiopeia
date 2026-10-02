//! Field names a mapping author writes inside a Tera expression that Tera does not read as one
//! field.
//!
//! Two spellings read a single column in a plain `{{ name }}` reference but mean something else, or
//! nothing, inside an expression or statement:
//!
//! - a hyphenated name such as `station-id`, which Tera parses as the subtraction `station - id` of
//!   two (usually undefined) variables, so it registers fine and then fails or misbehaves on every
//!   record;
//! - a name with a non-ASCII letter such as `čas`, which Tera's lexer, accepting only ASCII
//!   identifiers, rejects as an unexpected character.
//!
//! Both are found by walking the [`Lexer`]'s lexemes, so string literals, comments, raw blocks, and
//! literal text outside the delimiters are never inspected.

use crate::template::lexer::{Lexeme, LexemeKind, Lexer};
use derive_more::Display;

/// A field name as the author wrote it inside an expression, with the dotted path leading to it.
///
/// The text is a fragment of the template, kept only to be quoted back in a diagnostic, so it is
/// plain text rather than a field path: it never reaches a record lookup.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
#[display("{}", self.written())]
pub struct WrittenReference {
    /// The dotted path the name is read under, such as `ulica` in `ulica.številka`, or `None` for a
    /// name read from the record itself.
    parent: Option<String>,
    /// The name, such as `station-id` or `čas`.
    name: String,
}

impl WrittenReference {
    /// The reference exactly as written: `station-id`, or `ulica.številka`.
    #[must_use]
    pub fn written(&self) -> String {
        match &self.parent {
            Some(parent) => format!("{parent}.{}", self.name),
            None => self.name.clone(),
        }
    }

    /// The bracketed spelling that reads the name as one key: `this['station-id']`, or
    /// `ulica['številka']`.
    #[must_use]
    pub fn as_key_lookup(&self) -> String {
        format!("{}['{}']", self.parent.as_deref().unwrap_or("this"), self.name)
    }

    /// The spelling that keeps Tera's reading of every hyphen as a subtraction: `station - id`.
    #[must_use]
    pub fn as_subtraction(&self) -> String {
        self.written().replace('-', " - ")
    }

    /// The reference whose name spans `lexemes[first..=last]`, read under the dotted path of
    /// identifiers immediately before it.
    fn spanning(source: &str, lexemes: &[Lexeme<'_>], first: usize, last: usize) -> WrittenReference {
        let mut start = first;
        while start >= 2
            && lexemes[start - 1].is_symbol(".")
            && lexemes[start - 2].kind == LexemeKind::Identifier
            && lexemes[start - 2].touches(&lexemes[start - 1])
            && lexemes[start - 1].touches(&lexemes[start])
        {
            start -= 2;
        }

        WrittenReference {
            parent: (start < first).then(|| source[lexemes[start].offset..lexemes[first - 1].offset].to_string()),
            name: source[lexemes[first].offset..lexemes[last].end()].to_string(),
        }
    }
}

/// The first name in an expression or statement body that joins identifiers with a hyphen and no
/// whitespace, such as `station-id` or `a-b-c`.
///
/// A hyphen with whitespace on either side (`a - b`, `a -b`, `a- b`) is a deliberate operator, and
/// one next to a number (`n-1`, `1-n`) is plain arithmetic; neither is reported.
pub(crate) fn ambiguous_hyphen(source: &str) -> Option<WrittenReference> {
    let lexemes = lexemes(source);

    (0..lexemes.len()).find_map(|first| {
        let mut last = first;
        while let [identifier, hyphen, next, ..] = &lexemes[last..]
            && identifier.kind == LexemeKind::Identifier
            && hyphen.is_symbol("-")
            && next.kind == LexemeKind::Identifier
            && identifier.touches(hyphen)
            && hyphen.touches(next)
        {
            last += 2;
        }
        (last > first).then(|| WrittenReference::spanning(source, &lexemes, first, last))
    })
}

/// The first identifier in an expression or statement body that is not ASCII, such as `čas`.
pub(crate) fn non_ascii_identifier(source: &str) -> Option<WrittenReference> {
    let lexemes = lexemes(source);

    lexemes
        .iter()
        .position(|lexeme| lexeme.kind == LexemeKind::Identifier && !lexeme.text.is_ascii())
        .map(|index| WrittenReference::spanning(source, &lexemes, index, index))
}

/// The lexemes of `source` up to the end, or up to the point where it stops lexing; what lies past
/// a malformed region is Tera's to report.
fn lexemes(source: &str) -> Vec<Lexeme<'_>> {
    Lexer::new(source).map_while(Result::ok).collect()
}

#[cfg(test)]
mod tests {
    use crate::template::identifier_lint::{ambiguous_hyphen, non_ascii_identifier};

    fn hyphenated(source: &str) -> Option<String> {
        ambiguous_hyphen(source).map(|reference| reference.written())
    }

    fn non_ascii(source: &str) -> Option<String> {
        non_ascii_identifier(source).map(|reference| reference.written())
    }

    #[test]
    fn an_unspaced_hyphen_between_identifiers_is_ambiguous_in_any_body() {
        for (source, token) in [
            ("{{ station-id }}", "station-id"),
            ("{{ station-id | upper }}", "station-id"),
            ("{% if station-id %}x{% endif %}", "station-id"),
            ("{{ a-b-c }}", "a-b-c"),
            ("{{ x ~ čas-ura }}", "čas-ura"),
            ("{{-a-b-}}", "a-b"),
        ] {
            assert_eq!(hyphenated(source).as_deref(), Some(token), "{source}");
        }
    }

    #[test]
    fn a_spaced_hyphen_or_a_number_beside_it_is_arithmetic() {
        for source in ["{{ a - b }}", "{{ a -b }}", "{{ a- b }}", "{{ n-1 }}", "{{ 1-n }}", "{{ a-}}", "{{ a -}}"] {
            assert_eq!(hyphenated(source), None, "{source}");
        }
    }

    #[test]
    fn a_hyphen_inside_a_string_literal_is_not_ambiguous() {
        for source in [
            "{{ this['a-b'] }}",
            r#"{{ "a-b" }}"#,
            "{{ 'x-y' | upper }}",
            "{{ `a-b` }}",
            r#"{{ a | replace(from="a-b", to='c-d') }}"#,
        ] {
            assert_eq!(hyphenated(source), None, "{source}");
        }
    }

    #[test]
    fn a_hyphen_outside_the_delimiters_is_literal_text() {
        for source in [
            "Station-{{ id }}",
            "{{ a }}-{{ b }}",
            "{# a-b #}{{ a }}",
            "{% raw %}{{ a-b }}{% endraw %}{{ c }}",
        ] {
            assert_eq!(hyphenated(source), None, "{source}");
        }
    }

    #[test]
    fn a_hyphenated_name_under_a_path_is_read_under_that_path() {
        let reference = ambiguous_hyphen("{{ props.station-id }}").unwrap();

        assert_eq!(reference.written(), "props.station-id");
        assert_eq!(reference.as_key_lookup(), "props['station-id']");
        assert_eq!(reference.as_subtraction(), "props.station - id");
    }

    #[test]
    fn a_top_level_hyphenated_name_is_read_from_the_record() {
        let reference = ambiguous_hyphen("{{ a-b-c }}").unwrap();

        assert_eq!(reference.as_key_lookup(), "this['a-b-c']");
        assert_eq!(reference.as_subtraction(), "a - b - c");
    }

    #[test]
    fn a_non_ascii_identifier_in_an_expression_is_found() {
        for (source, identifier) in [
            ("{{ čas | upper }}", "čas"),
            ("{% if čas %}{{ čas }}{% endif %}", "čas"),
            ("{{ Città ~ '-' }}", "Città"),
            ("{{ ulica.številka | upper }}", "ulica.številka"),
        ] {
            assert_eq!(non_ascii(source).as_deref(), Some(identifier), "{source}");
        }
        assert_eq!(
            non_ascii_identifier("{{ ulica.številka | upper }}").unwrap().as_key_lookup(),
            "ulica['številka']"
        );
        assert_eq!(non_ascii_identifier("{{ čas | upper }}").unwrap().as_key_lookup(), "this['čas']");
    }

    #[test]
    fn non_ascii_text_in_a_string_literal_or_outside_the_delimiters_is_not_an_identifier() {
        for source in [
            r#"{{ "čas" }}"#,
            r#"{{ x | replace(from="č", to="c") }}"#,
            "Čas: {{ x | upper }}",
            "{# čas #}{{ x | upper }}",
            "{% raw %}{{ čas }}{% endraw %}",
        ] {
            assert_eq!(non_ascii(source), None, "{source}");
        }
    }
}
