/// The fewest whitespace-separated words a cell needs to read as a sentence. Fewer is as likely a
/// name with an abbreviation, such as `Acme Inc.`.
const MIN_SENTENCE_WORDS: usize = 3;

/// Whether `cell` reads as a sentence of prose: several words, closed by a full stop, a question
/// mark, or an exclamation mark.
pub fn is_sentence(cell: &str) -> bool {
    let cell = cell.trim();
    cell.ends_with(['.', '?', '!']) && cell.split_whitespace().count() >= MIN_SENTENCE_WORDS
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::sentence::is_sentence;

    #[test]
    fn several_words_closed_by_a_full_stop_are_a_sentence() {
        assert!(is_sentence("More text here."));
        assert!(is_sentence("Is this a question?"));
    }

    #[test]
    fn a_name_with_an_abbreviation_is_no_sentence() {
        assert!(!is_sentence("Acme Inc."));
    }

    #[test]
    fn words_without_closing_punctuation_are_no_sentence() {
        assert!(!is_sentence("Ada Lovelace of London"));
    }
}
