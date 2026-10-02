//! Turns Tera's refusal to register a template into the [`TemplateCompileError`] that says what the
//! author should change.
//!
//! Tera reports the first thing it could not parse, positioned in a template it knows only by its
//! registration digest. The common causes are recognised here from the template text itself, in the
//! order they explain Tera's report: a name Tera cannot read, then a delimiter or block left open,
//! then a filter, function, or test that is not registered. Anything else is reported as a syntax
//! error at the position Tera points at.

use crate::template::{
    TemplateSource,
    block_structure::unclosed_block,
    compile_error::TemplateCompileError,
    engine_report::EngineReport,
    identifier_lint::non_ascii_identifier,
    lexer::{LexError, Lexer},
    source_position::SourcePosition,
    vocabulary::Vocabulary,
};

/// Explains why Tera refused to register `template` as written.
pub(crate) fn diagnose(template: &TemplateSource, engine: tera::Error, vocabulary: &Vocabulary) -> TemplateCompileError {
    let text = template.as_str();
    let source = EngineReport::located(text, engine);
    // The error outlives the mapping the expression is borrowed from.
    let template = template.clone();

    if let Some(reference) = non_ascii_identifier(text) {
        return TemplateCompileError::NonAsciiIdentifier { template, reference, source };
    }
    if let Some(LexError::Unclosed { opener, offset }) = Lexer::new(text).find_map(Result::err) {
        return TemplateCompileError::UnclosedDelimiter {
            position: SourcePosition::at_offset(text, offset),
            template,
            opener,
            source,
        };
    }
    if let Some(block) = unclosed_block(text) {
        return TemplateCompileError::UnclosedBlock {
            position: SourcePosition::at_offset(text, block.offset),
            template,
            tag: block.tag,
            source,
        };
    }
    if let Some((kind, name)) = source.unknown_callable() {
        let name = name.to_string();
        return match vocabulary.closest(kind, &name) {
            Some(suggestion) => TemplateCompileError::MisspelledCallable {
                template,
                kind,
                name,
                suggestion,
                source,
            },
            None => TemplateCompileError::UnknownCallable { template, kind, name, source },
        };
    }

    TemplateCompileError::Syntax { template, source }
}

#[cfg(test)]
mod tests {
    use crate::template::{
        TemplateSource,
        block_structure::BlockTag,
        compile_diagnosis::diagnose,
        compile_error::TemplateCompileError,
        lexer::Opener,
        vocabulary::{CallableKind, Vocabulary},
    };
    use tera::Tera;

    /// The diagnosis of Tera's refusal to register `source`.
    fn diagnosis(source: &str) -> TemplateCompileError {
        let engine = Tera::default().add_raw_template("tpl_digest", source).unwrap_err();

        diagnose(&TemplateSource::new(source), engine, &Vocabulary::of_engine_builtins())
    }

    #[test]
    fn a_non_ascii_name_is_diagnosed_before_anything_else() {
        assert!(matches!(
            diagnosis("{{ čas | upper "),
            TemplateCompileError::NonAsciiIdentifier { reference, .. } if reference.written() == "čas"
        ));
    }

    #[test]
    fn an_unclosed_delimiter_is_diagnosed_with_its_position() {
        let error = diagnosis("Station-{{ id | upper ");

        assert!(matches!(&error, TemplateCompileError::UnclosedDelimiter { opener: Opener::Expression, position, .. } if position.column() == 9));
        assert_eq!(
            error.to_string(),
            "`{{` opened at column 9 of `Station-{{ id | upper ` is never closed: close it with `}}`"
        );
    }

    #[test]
    fn an_unclosed_comment_or_raw_block_is_diagnosed() {
        assert!(matches!(
            diagnosis("{# note {{ a }}"),
            TemplateCompileError::UnclosedDelimiter { opener: Opener::Comment, .. }
        ));
        assert!(matches!(
            diagnosis("{% raw %}{{ a }}"),
            TemplateCompileError::UnclosedDelimiter { opener: Opener::Raw, .. }
        ));
    }

    #[test]
    fn a_missing_end_tag_names_the_open_block_and_its_end_tag() {
        let error = diagnosis("{% if t %}{{ t | upper }}");

        assert!(matches!(&error, TemplateCompileError::UnclosedBlock { tag: BlockTag::If, .. }));
        assert_eq!(
            error.to_string(),
            "`{% if %}` opened at column 1 of `{% if t %}{{ t | upper }}` is never closed: add `{% endif %}`"
        );
    }

    #[test]
    fn a_misspelt_filter_suggests_the_registered_one() {
        let error = diagnosis("{{ t | uper }}");

        assert!(matches!(&error, TemplateCompileError::MisspelledCallable { kind: CallableKind::Filter, name, suggestion: "upper", .. } if name == "uper"));
        assert_eq!(error.to_string(), "Unknown filter `uper` in `{{ t | uper }}`: did you mean `upper`?");
    }

    #[test]
    fn an_unknown_filter_far_from_every_registered_one_is_named_without_a_suggestion() {
        let error = diagnosis("{{ t | no_such_filter }}");

        assert!(matches!(&error, TemplateCompileError::UnknownCallable { kind: CallableKind::Filter, name, .. } if name == "no_such_filter"));
        assert_eq!(error.to_string(), "Unknown filter `no_such_filter` in `{{ t | no_such_filter }}`");
    }

    #[test]
    fn an_unknown_function_is_named() {
        assert!(matches!(
            diagnosis("{{ rang(end=3) }}"),
            TemplateCompileError::MisspelledCallable {
                kind: CallableKind::Function,
                suggestion: "range",
                ..
            }
        ));
    }

    #[test]
    fn any_other_refusal_is_a_syntax_error_at_teras_position() {
        let error = diagnosis("{{ a b }}");

        assert!(matches!(error, TemplateCompileError::Syntax { .. }));
        assert_eq!(error.to_string(), "`{{ a b }}` is not valid template syntax at column 6");
    }
}
