//! A lexer over a template's source that reads it the way Tera's own lexer does: literal text,
//! `{# #}` comments, and `{% raw %}` blocks are skipped whole, and only the bodies of `{{ }}`
//! expressions and `{% %}` statements are split into lexemes.
//!
//! It exists so a template can be checked, and a failure explained, in terms of what the author
//! wrote: Tera reports neither which name it could not read nor which delimiter is unclosed in a
//! form a mapping author can act on. It is not a parser and does not validate anything Tera checks;
//! it only has to agree with Tera on where expression bodies, string literals, and identifiers start
//! and end. Identifiers follow [`identifier`](crate::template::identifier) rather than Tera's ASCII
//! grammar, so a name Tera cannot read is still lexed whole and can be named.

use crate::template::identifier::identifier_length;
use derive_more::Display;

/// The opening delimiter of a region of a template that is not literal text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum Opener {
    /// `{{`, which opens an expression.
    #[display("`{{{{`")]
    Expression,

    /// `{%`, which opens a statement.
    #[display("`{{%`")]
    Statement,

    /// `{#`, which opens a comment.
    #[display("`{{#`")]
    Comment,

    /// `{% raw %}`, which opens a block of literal text.
    #[display("`{{% raw %}}`")]
    Raw,
}

impl Opener {
    /// The delimiter that closes what this opener opens.
    #[must_use]
    pub const fn closer(self) -> &'static str {
        match self {
            Opener::Expression => "}}",
            Opener::Statement => "%}",
            Opener::Comment => "#}",
            Opener::Raw => "{% endraw %}",
        }
    }
}

/// What one lexeme is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LexemeKind {
    /// The `{{` or `{%` opening a body, a whitespace-control `-` included.
    Open(Opener),
    /// The `}}` or `%}` closing a body, a whitespace-control `-` included.
    Close,
    /// An identifier.
    Identifier,
    /// An integer or decimal literal.
    Number,
    /// A string literal, quotes included.
    StringLiteral,
    /// Any other single character: an operator, a bracket, or punctuation.
    Symbol,
}

/// One lexeme of a template, located by its byte offset in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Lexeme<'a> {
    /// What the lexeme is.
    pub(crate) kind: LexemeKind,
    /// The lexeme's text, exactly as written.
    pub(crate) text: &'a str,
    /// The byte offset of the lexeme's first character in the template.
    pub(crate) offset: usize,
}

impl Lexeme<'_> {
    /// The byte offset just past the lexeme.
    pub(crate) const fn end(&self) -> usize {
        self.offset + self.text.len()
    }

    /// Whether `next` starts exactly where this lexeme ends, with no whitespace between them.
    pub(crate) const fn touches(&self, next: &Lexeme<'_>) -> bool {
        self.end() == next.offset
    }

    /// Whether this lexeme is the one-character symbol `symbol`.
    pub(crate) fn is_symbol(&self, symbol: &str) -> bool {
        self.kind == LexemeKind::Symbol && self.text == symbol
    }
}

/// Why lexing stopped before the end of the template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LexError {
    /// A delimiter or raw block is never closed.
    Unclosed {
        /// What was opened.
        opener: Opener,
        /// The byte offset of the opening delimiter.
        offset: usize,
    },

    /// A string literal is never closed.
    UnterminatedString,
}

/// Where the lexer is in the template.
#[derive(Debug, Clone, Copy)]
enum State {
    /// In literal text, looking for the next opening delimiter.
    Text,
    /// Inside the body that `opener`, at byte `offset`, opened.
    Body { opener: Opener, offset: usize },
    /// Done, at the end of the template or after an error.
    Finished,
}

/// The lexemes of a template, in source order, ending early with a [`LexError`] when the template
/// is malformed in a way that stops Tera's lexer too.
pub(crate) struct Lexer<'a> {
    source: &'a str,
    position: usize,
    state: State,
}

impl<'a> Lexer<'a> {
    /// A lexer at the start of `source`.
    pub(crate) const fn new(source: &'a str) -> Lexer<'a> {
        Lexer {
            source,
            position: 0,
            state: State::Text,
        }
    }

    /// Skips literal text, comments, and raw blocks up to the next body, opening it.
    fn next_in_text(&mut self) -> Option<Result<Lexeme<'a>, LexError>> {
        loop {
            let Some((start, marker)) = next_opening(self.source, self.position) else {
                self.state = State::Finished;
                return None;
            };
            let after = start + 2 + usize::from(self.source.as_bytes().get(start + 2) == Some(&b'-'));

            match marker {
                Marker::Expression => return Some(Ok(self.open(Opener::Expression, start, after))),
                Marker::Statement => match raw_block_end(self.source, after) {
                    RawBlock::Absent => return Some(Ok(self.open(Opener::Statement, start, after))),
                    RawBlock::Closed { end } => self.position = end,
                    RawBlock::Unclosed => return Some(Err(self.fail(Opener::Raw, start))),
                },
                // Tera ends a comment at the first `#}`, whatever lies between.
                Marker::Comment => match self.source[after..].find("#}") {
                    Some(length) => self.position = after + length + 2,
                    None => return Some(Err(self.fail(Opener::Comment, start))),
                },
            }
        }
    }

    /// Reads the next lexeme of the body `opener` opened at byte `offset`.
    fn next_in_body(&mut self, opener: Opener, offset: usize) -> Result<Lexeme<'a>, LexError> {
        let rest = &self.source[self.position..];
        let skipped = rest.len() - rest.trim_start_matches(|character: char| character.is_ascii_whitespace()).len();
        self.position += skipped;
        let rest = &self.source[self.position..];

        let closer = opener.closer();
        let close_length = if rest.strip_prefix('-').is_some_and(|after| after.starts_with(closer)) {
            Some(closer.len() + 1)
        } else if rest.starts_with(closer) {
            Some(closer.len())
        } else {
            None
        };
        if let Some(length) = close_length {
            self.state = State::Text;
            return Ok(self.take(LexemeKind::Close, length));
        }

        let Some(first) = rest.chars().next() else {
            return Err(self.fail(opener, offset));
        };
        match first {
            '\'' | '"' | '`' => {
                if let Some(length) = string_literal_length(rest, first) {
                    Ok(self.take(LexemeKind::StringLiteral, length))
                } else {
                    self.state = State::Finished;
                    Err(LexError::UnterminatedString)
                }
            }
            digit if digit.is_ascii_digit() => Ok(self.take(LexemeKind::Number, number_length(rest))),
            other => match identifier_length(rest) {
                0 => Ok(self.take(LexemeKind::Symbol, other.len_utf8())),
                length => Ok(self.take(LexemeKind::Identifier, length)),
            },
        }
    }

    /// Opens the body `opener` starts at byte `start`, its delimiter ending at byte `after`.
    fn open(&mut self, opener: Opener, start: usize, after: usize) -> Lexeme<'a> {
        self.state = State::Body { opener, offset: start };
        self.position = after;
        Lexeme {
            kind: LexemeKind::Open(opener),
            text: &self.source[start..after],
            offset: start,
        }
    }

    /// Takes the next `length` bytes as one lexeme of `kind`.
    fn take(&mut self, kind: LexemeKind, length: usize) -> Lexeme<'a> {
        let offset = self.position;
        self.position += length;
        Lexeme {
            kind,
            text: &self.source[offset..self.position],
            offset,
        }
    }

    /// Stops lexing because `opener`, at byte `offset`, is never closed.
    const fn fail(&mut self, opener: Opener, offset: usize) -> LexError {
        self.state = State::Finished;
        LexError::Unclosed { opener, offset }
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Result<Lexeme<'a>, LexError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.state {
            State::Text => self.next_in_text(),
            State::Body { opener, offset } => Some(self.next_in_body(opener, offset)),
            State::Finished => None,
        }
    }
}

/// Whether a `{%` opens a raw block, and where that block ends.
enum RawBlock {
    /// The tag is not `raw`.
    Absent,
    /// The raw block ends just before byte `end`.
    Closed { end: usize },
    /// The raw block has no `{% endraw %}`.
    Unclosed,
}

/// Reads the statement whose body starts at byte `body` as a `{% raw %}` tag and finds its
/// `{% endraw %}`, exactly as Tera's lexer does.
fn raw_block_end(source: &str, body: usize) -> RawBlock {
    let Some(tag_length) = named_tag_length(&source[body..], "raw") else {
        return RawBlock::Absent;
    };

    let mut search = body + tag_length;
    while let Some(found) = source[search..].find("{%") {
        search += found + 2;
        if let Some(length) = named_tag_length(&source[search..], "endraw") {
            return RawBlock::Closed { end: search + length };
        }
    }
    RawBlock::Unclosed
}

/// The length of the rest of a statement whose body `text` is exactly the tag `name`: an optional
/// `-`, ASCII whitespace, the name, ASCII whitespace, an optional `-`, and `%}`.
fn named_tag_length(text: &str, name: &str) -> Option<usize> {
    let is_space = |character: char| character.is_ascii_whitespace();
    let rest = text.strip_prefix('-').unwrap_or(text).trim_start_matches(is_space);
    let rest = rest.strip_prefix(name)?.trim_start_matches(is_space);
    let rest = rest.strip_prefix('-').unwrap_or(rest).strip_prefix("%}")?;

    Some(text.len() - rest.len())
}

/// The two-character delimiters that end a run of literal text.
#[derive(Debug, Clone, Copy)]
enum Marker {
    /// `{{`.
    Expression,
    /// `{%`.
    Statement,
    /// `{#`.
    Comment,
}

/// The byte offset of the next `{{`, `{%`, or `{#` at or after `from`, and which one it is.
fn next_opening(source: &str, from: usize) -> Option<(usize, Marker)> {
    source.as_bytes()[from..].windows(2).enumerate().find_map(|(index, pair)| {
        let marker = match pair {
            [b'{', b'{'] => Marker::Expression,
            [b'{', b'%'] => Marker::Statement,
            [b'{', b'#'] => Marker::Comment,
            [..] => return None,
        };
        Some((from + index, marker))
    })
}

/// The length of the string literal `text` starts with, quotes included, or `None` when it is never
/// closed. A backslash escapes the character after it, as in Tera.
fn string_literal_length(text: &str, quote: char) -> Option<usize> {
    let mut escaped = false;
    text.char_indices().skip(1).find_map(|(index, character)| {
        if escaped {
            escaped = false;
            None
        } else if character == '\\' {
            escaped = true;
            None
        } else if character == quote {
            Some(index + quote.len_utf8())
        } else {
            None
        }
    })
}

/// The length of the number literal `text` starts with: digits with at most one `.`, as in Tera.
fn number_length(text: &str) -> usize {
    let mut seen_point = false;
    text.bytes()
        .take_while(|&byte| match byte {
            b'.' if !seen_point => {
                seen_point = true;
                true
            }
            other => other.is_ascii_digit(),
        })
        .count()
}

#[cfg(test)]
mod tests {
    use crate::template::lexer::{LexError, Lexeme, LexemeKind, Lexer, Opener};

    /// Every lexeme of `source` as `(kind, text)`, and how lexing ended.
    fn lex(source: &str) -> (Vec<(LexemeKind, &str)>, Option<LexError>) {
        let mut lexemes = Vec::new();
        for item in Lexer::new(source) {
            match item {
                Ok(Lexeme { kind, text, .. }) => lexemes.push((kind, text)),
                Err(error) => return (lexemes, Some(error)),
            }
        }
        (lexemes, None)
    }

    #[test]
    fn literal_text_yields_no_lexemes() {
        assert_eq!(lex("Station-1 }} %} #}"), (vec![], None));
    }

    #[test]
    fn an_expression_body_is_split_into_identifiers_symbols_and_literals() {
        let (lexemes, end) = lex("a{{ station-id | replace(from='-', to=\"_\") ~ 1.5 }}b");

        assert_eq!(end, None);
        assert_eq!(
            lexemes,
            [
                (LexemeKind::Open(Opener::Expression), "{{"),
                (LexemeKind::Identifier, "station"),
                (LexemeKind::Symbol, "-"),
                (LexemeKind::Identifier, "id"),
                (LexemeKind::Symbol, "|"),
                (LexemeKind::Identifier, "replace"),
                (LexemeKind::Symbol, "("),
                (LexemeKind::Identifier, "from"),
                (LexemeKind::Symbol, "="),
                (LexemeKind::StringLiteral, "'-'"),
                (LexemeKind::Symbol, ","),
                (LexemeKind::Identifier, "to"),
                (LexemeKind::Symbol, "="),
                (LexemeKind::StringLiteral, "\"_\""),
                (LexemeKind::Symbol, ")"),
                (LexemeKind::Symbol, "~"),
                (LexemeKind::Number, "1.5"),
                (LexemeKind::Close, "}}"),
            ]
        );
    }

    #[test]
    fn whitespace_control_dashes_belong_to_the_delimiters() {
        let (lexemes, _) = lex("{{- a-}}{%- if a -%}");

        assert_eq!(
            lexemes,
            [
                (LexemeKind::Open(Opener::Expression), "{{-"),
                (LexemeKind::Identifier, "a"),
                (LexemeKind::Close, "-}}"),
                (LexemeKind::Open(Opener::Statement), "{%-"),
                (LexemeKind::Identifier, "if"),
                (LexemeKind::Identifier, "a"),
                (LexemeKind::Close, "-%}"),
            ]
        );
    }

    #[test]
    fn a_closing_delimiter_inside_a_string_literal_does_not_close_the_body() {
        let (lexemes, end) = lex(r#"{{ a | replace(from="}}", to=`\``) }}"#);

        assert_eq!(end, None);
        assert!(lexemes.contains(&(LexemeKind::StringLiteral, r#""}}""#)));
        assert!(lexemes.contains(&(LexemeKind::StringLiteral, r"`\``")));
        assert_eq!(lexemes.last(), Some(&(LexemeKind::Close, "}}")));
    }

    #[test]
    fn non_ascii_names_are_lexed_whole() {
        let (lexemes, _) = lex("{{ Città ~ ulica.številka }}");

        assert!(lexemes.contains(&(LexemeKind::Identifier, "Città")));
        assert!(lexemes.contains(&(LexemeKind::Identifier, "številka")));
    }

    #[test]
    fn comments_and_raw_blocks_are_skipped() {
        let (lexemes, end) = lex("{# a-b #}{% raw %}{{ c-d }}{% endraw %}{{ e }}");

        assert_eq!(end, None);
        assert_eq!(
            lexemes,
            [
                (LexemeKind::Open(Opener::Expression), "{{"),
                (LexemeKind::Identifier, "e"),
                (LexemeKind::Close, "}}"),
            ]
        );
    }

    #[test]
    fn a_raw_block_with_whitespace_control_is_skipped() {
        assert_eq!(lex("{%- raw -%}{{ c-d }}{%- endraw -%}").0, vec![]);
    }

    #[test]
    fn an_unclosed_expression_names_its_opening_delimiter() {
        let (_, end) = lex("ab{{ t | upper ");

        assert_eq!(
            end,
            Some(LexError::Unclosed {
                opener: Opener::Expression,
                offset: 2
            })
        );
    }

    #[test]
    fn an_unclosed_statement_comment_or_raw_block_names_its_opener() {
        assert_eq!(
            lex("{% if a").1,
            Some(LexError::Unclosed {
                opener: Opener::Statement,
                offset: 0
            })
        );
        assert_eq!(
            lex("x {# note").1,
            Some(LexError::Unclosed {
                opener: Opener::Comment,
                offset: 2
            })
        );
        assert_eq!(
            lex("{% raw %}{{ a }}").1,
            Some(LexError::Unclosed {
                opener: Opener::Raw,
                offset: 0
            })
        );
    }

    #[test]
    fn an_unterminated_string_stops_lexing() {
        assert_eq!(lex("{{ 'abc }}").1, Some(LexError::UnterminatedString));
    }

    #[test]
    fn each_opener_renders_itself_and_names_its_closer() {
        assert_eq!(Opener::Expression.to_string(), "`{{`");
        assert_eq!(Opener::Statement.to_string(), "`{%`");
        assert_eq!(Opener::Comment.to_string(), "`{#`");
        assert_eq!(Opener::Raw.to_string(), "`{% raw %}`");
        assert_eq!(Opener::Raw.closer(), "{% endraw %}");
    }
}
