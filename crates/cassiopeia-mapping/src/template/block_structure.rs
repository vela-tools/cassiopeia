//! The nesting of a template's block statements, checked to explain a missing end tag.
//!
//! Tera reports a block left open, such as `{% if a %}` without its `{% endif %}`, only as an
//! unexpected end of input at the very end of the template. Pairing the opening and closing tags
//! names the block that is still open and the tag that closes it.

use crate::template::lexer::{Lexeme, LexemeKind, Lexer, Opener};
use strum::{Display, EnumString};

/// A statement that opens a block Tera requires to be closed by a matching end tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum BlockTag {
    /// `{% if %}`, closed by `{% endif %}`.
    If,
    /// `{% for %}`, closed by `{% endfor %}`.
    For,
    /// `{% filter %}`, closed by `{% endfilter %}`.
    Filter,
    /// `{% block %}`, closed by `{% endblock %}`.
    Block,
    /// `{% set name %}` capturing its body, closed by `{% endset %}`; `{% set name = value %}` opens
    /// no block.
    Set,
    /// `{% set_global name %}` capturing its body, closed by `{% endset %}`.
    SetGlobal,
}

impl BlockTag {
    /// The name of the tag that closes this block.
    #[must_use]
    pub const fn end_tag(self) -> &'static str {
        match self {
            BlockTag::If => "endif",
            BlockTag::For => "endfor",
            BlockTag::Filter => "endfilter",
            BlockTag::Block => "endblock",
            BlockTag::Set | BlockTag::SetGlobal => "endset",
        }
    }

    /// Whether a statement opening this tag with `body` (the lexemes after the tag name) opens a
    /// block; an assignment does not.
    fn opens_block(self, body: &[Lexeme<'_>]) -> bool {
        match self {
            BlockTag::If | BlockTag::For | BlockTag::Filter | BlockTag::Block => true,
            BlockTag::Set | BlockTag::SetGlobal => !body.iter().any(|lexeme| lexeme.is_symbol("=")),
        }
    }
}

/// A block statement that is never closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnclosedBlock {
    /// The statement that opens the block.
    pub(crate) tag: BlockTag,
    /// The byte offset of the statement's `{%`.
    pub(crate) offset: usize,
}

/// The innermost block left open at the end of `source`, or `None` when every block is closed, the
/// tags do not pair up, or the template does not lex to its end; in each of those cases a missing
/// end tag would not be the whole story.
pub(crate) fn unclosed_block(source: &str) -> Option<UnclosedBlock> {
    let lexemes: Vec<Lexeme<'_>> = Lexer::new(source).collect::<Result<_, _>>().ok()?;
    let mut open: Vec<UnclosedBlock> = Vec::new();

    for (index, lexeme) in lexemes.iter().enumerate() {
        let (LexemeKind::Open(Opener::Statement), Some(name)) = (lexeme.kind, lexemes.get(index + 1)) else {
            continue;
        };
        let rest = &lexemes[index + 2..];
        let body = &rest[..rest.iter().take_while(|inner| inner.kind != LexemeKind::Close).count()];

        if let Ok(tag) = name.text.parse::<BlockTag>()
            && tag.opens_block(body)
        {
            open.push(UnclosedBlock { tag, offset: lexeme.offset });
        } else if let Some(closed) = name.text.strip_prefix("end")
            && closed.parse::<BlockTag>().is_ok()
        {
            match open.pop() {
                Some(block) if block.tag.end_tag() == name.text => {}
                Some(_) | None => return None,
            }
        }
    }

    open.pop()
}

#[cfg(test)]
mod tests {
    use crate::template::block_structure::{BlockTag, UnclosedBlock, unclosed_block};

    #[test]
    fn a_conditional_without_its_end_tag_is_unclosed() {
        assert_eq!(
            unclosed_block("{% if t %}{{ t | upper }}"),
            Some(UnclosedBlock { tag: BlockTag::If, offset: 0 })
        );
    }

    #[test]
    fn the_innermost_open_block_is_reported() {
        assert_eq!(
            unclosed_block("{% if a %}{% for x in xs %}{{ x }}"),
            Some(UnclosedBlock {
                tag: BlockTag::For,
                offset: 10
            })
        );
    }

    #[test]
    fn closed_blocks_are_not_reported() {
        for source in [
            "{% if a %}{{ a }}{% elif b %}b{% else %}c{% endif %}",
            "{% for x in xs %}{{ x }}{% endfor %}",
            "{% set x = a %}{{ x }}",
            "{% set x %}{{ a }}{% endset %}{{ x }}",
            "{%- if a -%}{{ a }}{%- endif -%}",
        ] {
            assert_eq!(unclosed_block(source), None, "{source}");
        }
    }

    #[test]
    fn a_capturing_set_without_its_end_tag_is_unclosed() {
        assert_eq!(unclosed_block("{% set x %}{{ a }}").map(|block| block.tag), Some(BlockTag::Set));
    }

    #[test]
    fn mismatched_tags_are_not_explained_as_a_missing_end_tag() {
        assert_eq!(unclosed_block("{% if a %}{% for x in xs %}{{ x }}{% endif %}"), None);
        assert_eq!(unclosed_block("{{ a }}{% endif %}"), None);
    }

    #[test]
    fn a_template_that_does_not_lex_to_its_end_is_not_explained() {
        assert_eq!(unclosed_block("{% if a %}{{ a"), None);
    }

    #[test]
    fn each_tag_renders_its_name_and_names_its_end_tag() {
        assert_eq!(BlockTag::If.to_string(), "if");
        assert_eq!(BlockTag::SetGlobal.to_string(), "set_global");
        assert_eq!(BlockTag::SetGlobal.end_tag(), "endset");
        assert_eq!(BlockTag::For.end_tag(), "endfor");
    }
}
