use crate::template::{
    TemplateSource,
    block_structure::BlockTag,
    engine_report::EngineReport,
    identifier_lint::WrittenReference,
    lexer::Opener,
    source_position::SourcePosition,
    vocabulary::CallableKind,
};
use thiserror::Error;

/// A template expression that cannot be compiled, raised once while a mapping is loaded.
///
/// Kept apart from [`TemplateError`](crate::template::error::TemplateError), which describes a
/// compiled template failing against one record: a compile failure means no record could ever
/// render the template, so it rejects the mapping rather than dropping one attribute of one entity.
///
/// Each variant renders as one line that names the template as written and says what to change;
/// where Tera refused the template, Tera's own report, restated against that text, is the chained
/// cause.
#[derive(Debug, Error)]
pub enum TemplateCompileError {
    /// A name joins identifiers with an unspaced hyphen, which Tera would read as a subtraction.
    ///
    /// Tera accepts the template, so this is raised before Tera sees it: registered as written, it
    /// would fail or misbehave on every record instead of once, at load.
    #[error(
        "`{reference}` in `{template}` is ambiguous: write `{}` to read the field, or `{}` to subtract",
        .reference.as_key_lookup(),
        .reference.as_subtraction()
    )]
    AmbiguousHyphen {
        /// The expression exactly as the mapping document wrote it.
        template: TemplateSource,
        /// The hyphenated name.
        reference: WrittenReference,
    },

    /// A name inside an expression or statement is not ASCII, and Tera reads only ASCII names.
    #[error("`{reference}` in `{template}` is not a name Tera can read: write `{}` to read the field", .reference.as_key_lookup())]
    NonAsciiIdentifier {
        /// The expression exactly as the mapping document wrote it.
        template: TemplateSource,
        /// The non-ASCII name.
        reference: WrittenReference,
        /// Tera's report of the character it could not read.
        #[source]
        source: EngineReport,
    },

    /// A delimiter, comment, or raw block is opened and never closed.
    #[error("{opener} opened at {position} of `{template}` is never closed: close it with `{}`", .opener.closer())]
    UnclosedDelimiter {
        /// The expression exactly as the mapping document wrote it.
        template: TemplateSource,
        /// What is opened.
        opener: Opener,
        /// Where it is opened.
        position: SourcePosition,
        /// Tera's report of where the template ended.
        #[source]
        source: EngineReport,
    },

    /// A block statement is opened and never closed by its end tag.
    #[error("`{{% {tag} %}}` opened at {position} of `{template}` is never closed: add `{{% {} %}}`", .tag.end_tag())]
    UnclosedBlock {
        /// The expression exactly as the mapping document wrote it.
        template: TemplateSource,
        /// The statement opening the block.
        tag: BlockTag,
        /// Where the statement is.
        position: SourcePosition,
        /// Tera's report of where the template ended.
        #[source]
        source: EngineReport,
    },

    /// The template names a filter, function, or test that is not registered, and nothing registered
    /// is close to the name.
    #[error("Unknown {kind} `{name}` in `{template}`")]
    UnknownCallable {
        /// The expression exactly as the mapping document wrote it.
        template: TemplateSource,
        /// What kind of callable the template names.
        kind: CallableKind,
        /// The unregistered name, as Tera reports it; template text quoted back, not a lookup key.
        name: String,
        /// Tera's report.
        #[source]
        source: EngineReport,
    },

    /// The template names a filter, function, or test that is not registered, and a registered one
    /// is a likely misspelling of it.
    #[error("Unknown {kind} `{name}` in `{template}`: did you mean `{suggestion}`?")]
    MisspelledCallable {
        /// The expression exactly as the mapping document wrote it.
        template: TemplateSource,
        /// What kind of callable the template names.
        kind: CallableKind,
        /// The unregistered name, as Tera reports it; template text quoted back, not a lookup key.
        name: String,
        /// The registered name closest to it.
        suggestion: &'static str,
        /// Tera's report.
        #[source]
        source: EngineReport,
    },

    /// Tera refused the template for any other reason.
    #[error("`{template}` is not valid template syntax{}", .source.position().map_or_else(String::new, |position| format!(" at {position}")))]
    Syntax {
        /// The expression exactly as the mapping document wrote it.
        template: TemplateSource,
        /// Tera's report, saying what it expected.
        #[source]
        source: EngineReport,
    },
}

impl TemplateCompileError {
    /// The template that cannot be compiled, exactly as the mapping document wrote it.
    #[must_use]
    pub const fn template(&self) -> &TemplateSource {
        match self {
            TemplateCompileError::AmbiguousHyphen { template, .. }
            | TemplateCompileError::NonAsciiIdentifier { template, .. }
            | TemplateCompileError::UnclosedDelimiter { template, .. }
            | TemplateCompileError::UnclosedBlock { template, .. }
            | TemplateCompileError::UnknownCallable { template, .. }
            | TemplateCompileError::MisspelledCallable { template, .. }
            | TemplateCompileError::Syntax { template, .. } => template,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::template::{
        TemplateSource,
        compile_error::TemplateCompileError,
        engine_report::EngineReport,
        identifier_lint::{ambiguous_hyphen, non_ascii_identifier},
    };
    use std::error::Error;
    use tera::Tera;

    /// Tera's refusal of `source`, restated against it.
    fn refusal(source: &str) -> EngineReport {
        EngineReport::located(source, Tera::default().add_raw_template("tpl_digest", source).unwrap_err())
    }

    #[test]
    fn an_ambiguous_hyphen_names_the_token_and_both_spellings() {
        let error = TemplateCompileError::AmbiguousHyphen {
            template: TemplateSource::new("{{ station-id }}"),
            reference: ambiguous_hyphen("{{ station-id }}").unwrap(),
        };

        assert_eq!(
            error.to_string(),
            "`station-id` in `{{ station-id }}` is ambiguous: write `this['station-id']` to read the field, or `station - id` to subtract"
        );
        assert!(error.source().is_none());
    }

    #[test]
    fn a_non_ascii_identifier_names_the_identifier_and_its_bracketed_spelling_and_chains_teras_report() {
        let error = TemplateCompileError::NonAsciiIdentifier {
            template: TemplateSource::new("{{ čas | upper }}"),
            reference: non_ascii_identifier("{{ čas | upper }}").unwrap(),
            source: refusal("{{ čas | upper }}"),
        };

        assert_eq!(
            error.to_string(),
            "`čas` in `{{ čas | upper }}` is not a name Tera can read: write `this['čas']` to read the field"
        );
        assert_eq!(error.source().unwrap().to_string(), "Tera: Unexpected character at column 4");
    }

    #[test]
    fn a_generic_syntax_error_names_the_template_and_the_column_tera_points_at() {
        let error = TemplateCompileError::Syntax {
            template: TemplateSource::new("{{ a b }}"),
            source: refusal("{{ a b }}"),
        };

        assert!(error.to_string().starts_with("`{{ a b }}` is not valid template syntax at column 6"), "{error}");
        assert!(!error.source().unwrap().to_string().contains("tpl_"));
    }

    #[test]
    fn every_variant_exposes_the_template_as_written() {
        let error = TemplateCompileError::Syntax {
            template: TemplateSource::new("{{ a b }}"),
            source: refusal("{{ a b }}"),
        };

        assert_eq!(error.template(), &TemplateSource::new("{{ a b }}"));
    }
}
