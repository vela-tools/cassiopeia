use derive_more::Display;

/// A position in a template's source text, counted the way the author reads it: a 1-based line and
/// a 1-based column of characters, not bytes.
///
/// A position in a single-line template, which is nearly every mapping template, names only its
/// column, so a report never makes the author count lines in a one-line expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum SourcePosition {
    /// A position in a template that is one line long.
    #[display("column {column}")]
    SingleLine {
        /// The 1-based column, in characters.
        column: usize,
    },

    /// A position in a template spanning several lines.
    #[display("line {line}, column {column}")]
    MultiLine {
        /// The 1-based line.
        line: usize,
        /// The 1-based column, in characters.
        column: usize,
    },
}

impl SourcePosition {
    /// The position of the byte at `offset` in `source`.
    ///
    /// An offset past the end, or inside a multi-byte character, is clamped to the nearest character
    /// boundary at or before it, so a position is always one the author can find.
    #[must_use]
    pub fn at_offset(source: &str, offset: usize) -> SourcePosition {
        let mut boundary = offset.min(source.len());
        while !source.is_char_boundary(boundary) {
            boundary -= 1;
        }
        let before = &source[..boundary];
        let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);

        SourcePosition::in_source(source, before.matches('\n').count() + 1, before[line_start..].chars().count() + 1)
    }

    /// The position Tera reports as a 1-based line and a 0-based character column.
    #[must_use]
    pub fn from_engine(source: &str, line: usize, zero_based_column: usize) -> SourcePosition {
        SourcePosition::in_source(source, line, zero_based_column + 1)
    }

    /// The 1-based line.
    #[must_use]
    pub const fn line(&self) -> usize {
        match *self {
            SourcePosition::SingleLine { .. } => 1,
            SourcePosition::MultiLine { line, .. } => line,
        }
    }

    /// The 1-based column, in characters.
    #[must_use]
    pub const fn column(&self) -> usize {
        match *self {
            SourcePosition::SingleLine { column } | SourcePosition::MultiLine { column, .. } => column,
        }
    }

    /// The position at `line` and `column` of `source`, shaped by whether `source` spans one line.
    fn in_source(source: &str, line: usize, column: usize) -> SourcePosition {
        if source.contains('\n') {
            SourcePosition::MultiLine { line, column }
        } else {
            SourcePosition::SingleLine { column }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::template::source_position::SourcePosition;

    #[test]
    fn a_position_in_a_one_line_template_names_only_its_column() {
        let position = SourcePosition::at_offset("{{ t | upper ", 0);

        assert_eq!((position.line(), position.column()), (1, 1));
        assert_eq!(position.to_string(), "column 1");
    }

    #[test]
    fn columns_count_characters_rather_than_bytes() {
        let source = "{{ čas | upper }}";

        assert_eq!(SourcePosition::at_offset(source, source.find('|').unwrap()).column(), 8);
    }

    #[test]
    fn a_position_in_a_multi_line_template_names_its_line_too() {
        let source = "{% if a %}\n  {{ a";
        let position = SourcePosition::at_offset(source, source.rfind("{{").unwrap());

        assert_eq!((position.line(), position.column()), (2, 3));
        assert_eq!(position.to_string(), "line 2, column 3");
    }

    #[test]
    fn an_offset_inside_a_character_or_past_the_end_is_clamped() {
        assert_eq!(SourcePosition::at_offset("č", 1).column(), 1);
        assert_eq!(SourcePosition::at_offset("ab", 10).column(), 3);
    }

    #[test]
    fn an_engine_position_is_converted_to_a_one_based_column() {
        assert_eq!(SourcePosition::from_engine("{{ a b }}", 1, 5).to_string(), "column 6");
    }
}
