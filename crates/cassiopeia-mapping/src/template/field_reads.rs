//! The dotted field paths a template reads from the record, such as `address.city` in
//! `{{ address.city | upper }}`.
//!
//! Tera lists only the top-level variables a template reads, and its report of a nested lookup that
//! failed names only the last segment it tried (``Field `city` is not defined``), so the whole path
//! is read from the template as written. The paths are found by walking the [`Lexer`]'s lexemes:
//! a run of identifiers joined by dots with no whitespace, kept only when it starts at a variable
//! Tera reports the template reading, or at the record variable, so filter, function, and test
//! names, keyword arguments, keywords, and loop variables are never taken for a field.

use crate::template::{
    field_path::FieldPath,
    lexer::{Lexeme, LexemeKind, Lexer},
    resolver::RECORD_VARIABLE,
};

/// The dotted paths `source` reads from the record, in the order they first appear, each once.
///
/// `variables` are the top-level variables Tera reports the template reading. A path read through
/// the record variable, such as `this.address.city`, is the path beneath it, `address.city`; the
/// record variable alone, or followed by a bracketed key, reads no path this can name. A path stops
/// at a bracket, so `codes[0].name` reads `codes`.
pub(crate) fn field_reads(source: &str, variables: &[&str]) -> Vec<FieldPath> {
    let lexemes = Lexer::new(source).map_while(Result::ok).collect::<Vec<Lexeme<'_>>>();
    let mut paths: Vec<FieldPath> = Vec::new();

    for start in (0..lexemes.len()).filter(|&index| starts_a_path(&lexemes, index)) {
        let last = path_end(&lexemes, start);
        let written = &source[lexemes[start].offset..lexemes[last].end()];
        let path = match written.split_once('.') {
            Some((RECORD_VARIABLE, beneath)) => beneath,
            Some((head, _)) if variables.contains(&head) => written,
            None if written != RECORD_VARIABLE && variables.contains(&written) => written,
            Some(_) | None => continue,
        };
        if !paths.iter().any(|known| known.as_str() == path) {
            paths.push(FieldPath::new(path));
        }
    }
    paths
}

/// Whether the identifier at `index` starts a path rather than continuing one after a dot.
fn starts_a_path(lexemes: &[Lexeme<'_>], index: usize) -> bool {
    lexemes[index].kind == LexemeKind::Identifier
        && !index
            .checked_sub(1)
            .is_some_and(|previous| lexemes[previous].is_symbol(".") && lexemes[previous].touches(&lexemes[index]))
}

/// The index of the last identifier in the path starting at `start`: each `.` and identifier that
/// follow with no whitespace between them extend it.
fn path_end(lexemes: &[Lexeme<'_>], start: usize) -> usize {
    let mut last = start;
    while let [identifier, dot, next, ..] = &lexemes[last..]
        && dot.is_symbol(".")
        && next.kind == LexemeKind::Identifier
        && identifier.touches(dot)
        && dot.touches(next)
    {
        last += 2;
    }
    last
}

#[cfg(test)]
mod tests {
    use crate::template::{field_path::FieldPath, field_reads::field_reads};
    use pretty_assertions::assert_eq;

    fn reads(source: &str, variables: &[&str]) -> Vec<String> {
        field_reads(source, variables).iter().map(FieldPath::to_string).collect()
    }

    #[test]
    fn a_dotted_reference_is_read_whole() {
        assert_eq!(reads("{{ address.street.name | upper }}", &["address"]), ["address.street.name"]);
    }

    #[test]
    fn a_top_level_reference_is_its_own_path() {
        assert_eq!(reads("{{ n + 1 }}", &["n"]), ["n"]);
    }

    #[test]
    fn paths_are_listed_in_the_order_they_first_appear_each_once() {
        assert_eq!(reads("{% if a.x %}{{ a.x }}-{{ b }}{% endif %}{{ a.y }}", &["a", "b"]), ["a.x", "b", "a.y"]);
    }

    #[test]
    fn filter_names_keyword_arguments_and_keywords_are_not_read() {
        assert_eq!(
            reads("{% if not a.x and b is defined %}{{ a.x | split(pat=' ') | first }}{% endif %}", &["a", "b"]),
            ["a.x", "b"]
        );
    }

    #[test]
    fn a_loop_variable_is_not_read() {
        assert_eq!(reads("{% for item in items %}{{ item.name }}{% endfor %}", &["items"]), ["items"]);
    }

    #[test]
    fn a_path_through_the_record_variable_is_the_path_beneath_it() {
        assert_eq!(reads("{{ this.address.city }}", &["this"]), ["address.city"]);
    }

    #[test]
    fn the_record_variable_alone_or_with_a_bracketed_key_reads_no_path() {
        assert_eq!(reads("{{ this['station-id'].x }}", &["this"]), Vec::<String>::new());
        assert_eq!(reads("{{ this | length }}", &["this"]), Vec::<String>::new());
    }

    #[test]
    fn a_path_stops_at_a_bracket() {
        assert_eq!(reads("{{ codes[0].name }}", &["codes"]), ["codes"]);
    }

    #[test]
    fn a_dot_after_whitespace_does_not_join_a_path() {
        assert_eq!(reads("{{ a .x }}", &["a"]), ["a"]);
    }

    #[test]
    fn text_outside_the_delimiters_and_string_literals_are_not_read() {
        assert_eq!(reads("a.x {{ b | default(value='c.d') }}", &["b"]), ["b"]);
    }
}
