use crate::csv::{column_key::ColumnKey, column_number::ColumnNumber, renamed_column::RenamedColumn};
use ::csv::StringRecord;
use std::collections::HashMap;

/// The record key of every column of a CSV header, no two alike, and the columns that had to be
/// renamed to make them so.
///
/// A column is keyed by its cleaned header, or by its zero-based position when the header is blank.
/// Two columns can ask for the same key: a header repeats a name, a stacked header joins to the same
/// name twice, or a blank header's position equals another column's name. One of them keeps the key
/// and every other one is keyed `<key>@<column number>` instead, so no column's values are lost. A
/// named column keeps the key over a blank one, then the leftmost column wins. A generated key never
/// takes a key some header asks for or another generated key: while it would, the `@<column number>`
/// suffix is appended again.
#[derive(Debug)]
pub(crate) struct ColumnKeys {
    /// The key of each column, in column order.
    keys: Vec<ColumnKey>,
    /// The columns read by a key other than the one their header asks for, in column order.
    renamed: Vec<RenamedColumn>,
}

/// Where the key a column asks for comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeySource {
    /// The column's header text.
    Header,
    /// The column's position, because its header is blank.
    Position,
}

/// The key a column asks for and where it comes from.
struct WantedKey {
    key: ColumnKey,
    source: KeySource,
}

/// The key a column is read by in place of the one it asks for, and the column holding that one.
struct Rename {
    holder: usize,
    key: String,
}

impl ColumnKeys {
    /// Resolves the key of every column of `headers`.
    pub(crate) fn from_headers(headers: &StringRecord) -> ColumnKeys {
        let wanted: Vec<WantedKey> = headers.iter().enumerate().map(|(index, header)| Self::wanted_key(index, header)).collect();
        let plan = Self::renames(&wanted);

        let mut renamed = Vec::new();
        let keys = wanted
            .into_iter()
            .zip(plan)
            .enumerate()
            .map(|(index, (wanted, rename))| match rename {
                None => wanted.key,
                Some(Rename { holder, key }) => {
                    let key = ColumnKey::from(key);
                    renamed.push(RenamedColumn {
                        wanted: wanted.key,
                        holder: ColumnNumber::of_index(holder),
                        column: ColumnNumber::of_index(index),
                        // The rename is reported apart from the records, which own their keys.
                        key: key.clone(),
                    });
                    key
                }
            })
            .collect();

        ColumnKeys { keys, renamed }
    }

    /// The key of each column, in column order.
    pub(crate) fn keys(&self) -> &[ColumnKey] {
        &self.keys
    }

    /// The columns read by a key other than the one their header asks for, in column order.
    pub(crate) fn renamed(&self) -> &[RenamedColumn] {
        &self.renamed
    }

    /// The key the column at `index` asks for: its cleaned header, or its zero-based position when
    /// the header is blank, as for a headerless source. A stacked header's key column often has no
    /// label at all.
    fn wanted_key(index: usize, header: &str) -> WantedKey {
        if header.trim().is_empty() {
            WantedKey {
                key: ColumnKey::from(index.to_string()),
                source: KeySource::Position,
            }
        } else {
            WantedKey {
                key: ColumnKey::from(Self::clean_header(header)),
                source: KeySource::Header,
            }
        }
    }

    /// Normalizes a header for use as a record key.
    ///
    /// A quoted header field may hold an embedded newline (RFC 4180 clause 2.6); folding it to a
    /// space keeps the key on one line. The header is otherwise preserved verbatim: its Unicode
    /// and punctuation are addressable directly from a mapping, so no transliteration is applied.
    fn clean_header(header: &str) -> String {
        header.replace('\n', " ")
    }

    /// The rename of each column that does not keep the key it asks for, in column order.
    fn renames(wanted: &[WantedKey]) -> Vec<Option<Rename>> {
        let mut holders: HashMap<&str, usize> = HashMap::with_capacity(wanted.len());
        for source in [KeySource::Header, KeySource::Position] {
            for (index, column) in wanted.iter().enumerate().filter(|(_, column)| column.source == source) {
                holders.entry(column.key.as_ref()).or_insert(index);
            }
        }

        let mut renames: Vec<Option<Rename>> = Vec::with_capacity(wanted.len());
        for (index, column) in wanted.iter().enumerate() {
            let rename = match holders.get(column.key.as_ref()) {
                Some(&holder) if holder != index => {
                    let suffix = format!("@{}", ColumnNumber::of_index(index));
                    let mut key = format!("{}{suffix}", column.key);
                    while holders.contains_key(key.as_str()) || renames.iter().flatten().any(|rename| rename.key == key) {
                        key.push_str(&suffix);
                    }
                    Some(Rename { holder, key })
                }
                Some(_) | None => None,
            };
            renames.push(rename);
        }
        renames
    }
}

#[cfg(test)]
mod tests {
    use crate::csv::{column_key::ColumnKey, column_keys::ColumnKeys, column_number::ColumnNumber, renamed_column::RenamedColumn};
    use ::csv::StringRecord;
    use pretty_assertions::assert_eq;

    fn keys_of(headers: &[&str]) -> ColumnKeys {
        ColumnKeys::from_headers(&StringRecord::from(headers.to_vec()))
    }

    fn texts(keys: &ColumnKeys) -> Vec<&str> {
        keys.keys().iter().map(AsRef::as_ref).collect()
    }

    fn renamed(wanted: &str, holder: usize, column: usize, key: &str) -> RenamedColumn {
        RenamedColumn {
            wanted: ColumnKey::from(wanted.to_owned()),
            holder: ColumnNumber::of_index(holder),
            column: ColumnNumber::of_index(column),
            key: ColumnKey::from(key.to_owned()),
        }
    }

    #[test]
    fn a_header_without_repeats_keeps_every_key_and_renames_nothing() {
        let keys = keys_of(&["id", "NO2\nHMW", "", "O3"]);

        assert_eq!(texts(&keys), ["id", "NO2 HMW", "2", "O3"]);
        assert_eq!(keys.renamed(), []);
    }

    #[test]
    fn a_repeated_header_keeps_its_first_column_and_keys_the_repeat_by_its_column_number() {
        let keys = keys_of(&["id", "value", "value"]);

        assert_eq!(texts(&keys), ["id", "value", "value@3"]);
        assert_eq!(keys.renamed(), [renamed("value", 1, 2, "value@3")]);
    }

    #[test]
    fn every_repeat_of_a_header_is_renamed_after_its_own_column() {
        let keys = keys_of(&["id", "value", "value", "value"]);

        assert_eq!(texts(&keys), ["id", "value", "value@3", "value@4"]);
        assert_eq!(keys.renamed(), [renamed("value", 1, 2, "value@3"), renamed("value", 1, 3, "value@4")]);
    }

    #[test]
    fn a_named_column_keeps_its_key_over_a_blank_header_keyed_by_the_same_position() {
        let keys = keys_of(&["", "name", "", "2"]);

        assert_eq!(texts(&keys), ["0", "name", "2@3", "2"]);
        assert_eq!(keys.renamed(), [renamed("2", 3, 2, "2@3")]);
    }

    #[test]
    fn a_generated_key_that_a_header_already_names_takes_the_suffix_again() {
        let keys = keys_of(&["id", "value", "value", "value@3"]);

        assert_eq!(texts(&keys), ["id", "value", "value@3@3", "value@3"]);
        assert_eq!(keys.renamed(), [renamed("value", 1, 2, "value@3@3")]);
    }

    #[test]
    fn a_generated_key_never_matches_another_generated_key() {
        // Column 3 repeats `a` and becomes `a@3`; column 4 repeats `a@3` itself, which column 3's
        // generated key now also holds.
        let keys = keys_of(&["a", "a@3", "a", "a@3"]);

        assert_eq!(texts(&keys), ["a", "a@3", "a@3@3", "a@3@4"]);
        assert_eq!(keys.renamed(), [renamed("a", 0, 2, "a@3@3"), renamed("a@3", 1, 3, "a@3@4")]);
    }
}
