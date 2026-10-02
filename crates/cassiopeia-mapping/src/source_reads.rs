use crate::template::field_path::FieldPath;
use derive_more::Display;
use lazy_regex::regex;
use std::collections::BTreeSet;

/// The name the engine binds the whole record to, so a template reading it can reach any field.
const RECORD_VARIABLE: &str = "this";

/// The prefix of the engine's own variables, such as `__tera_context`, which dumps the whole context
/// and which the engine leaves out of the variables it reports a template reading.
const ENGINE_VARIABLE_PREFIX: &str = "__tera_";

/// One top-level key of a source record, as the source format names it: a field name, or the
/// zero-based column index a headerless record is keyed by.
#[derive(Debug, Clone, Display, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[display("{_0}")]
pub struct SourceKey(Box<str>);

impl SourceKey {
    /// Wraps a top-level key.
    #[must_use]
    pub fn new(key: &str) -> SourceKey {
        SourceKey(Box::from(key))
    }
}

/// The top-level source keys a compiled mapping's templates read, or every key when a template can
/// reach the whole record.
///
/// A record field the mapping never reads cannot change any entity the mapping emits, so this is what
/// tells a disagreement that loses output from one that does not. The set may be larger than what a
/// template reads at run time (every branch of a conditional counts, as does every name a template
/// assigns), never smaller: reading more than is needed only reports more, while missing a read would
/// hide a lost value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SourceReads {
    /// Exactly these top-level keys.
    Keys(BTreeSet<SourceKey>),
    /// Any key at all: a template reads the whole record, or the mapping has not been compiled yet
    /// and nothing narrower is known.
    #[default]
    Everything,
}

impl SourceReads {
    /// Reads no key at all, the starting point a mapping's templates are added to.
    #[must_use]
    pub const fn nothing() -> SourceReads {
        SourceReads::Keys(BTreeSet::new())
    }

    /// What a direct field reference reads: its first segment, or the whole record for the reference
    /// an attribute source uses to take the record as its value.
    #[must_use]
    pub fn of_field(path: &FieldPath) -> SourceReads {
        if path.is_whole_record() {
            return SourceReads::Everything;
        }
        SourceReads::Keys(BTreeSet::from([SourceKey::new(path.head())]))
    }

    /// What a template the engine renders reads, from the top-level variables the engine reports and
    /// the template's own source.
    ///
    /// The engine reports every name an expression loads, in every branch and filter argument, but
    /// leaves out loop variables, names a `set` assigns, and its own `__tera_` variables. Loop
    /// variables are bound for as long as they are visible, so leaving them out hides nothing. A name
    /// assigned inside a branch that does not run falls back to the record's field of that name, so
    /// every assigned name is counted as read too. Reading `this` or an engine variable reaches the
    /// whole record.
    #[must_use]
    pub fn of_engine_template<'a>(variables: impl IntoIterator<Item = &'a str>, source: &str) -> SourceReads {
        if source.contains(ENGINE_VARIABLE_PREFIX) {
            return SourceReads::Everything;
        }
        let mut keys = BTreeSet::new();
        for variable in variables {
            if variable == RECORD_VARIABLE {
                return SourceReads::Everything;
            }
            keys.insert(SourceKey::new(variable));
        }
        for assigned in regex!(r"\{%-?\s*set(?:_global)?\s+([A-Za-z_][A-Za-z0-9_]*)").captures_iter(source) {
            if let Some(name) = assigned.get(1) {
                keys.insert(SourceKey::new(name.as_str()));
            }
        }
        SourceReads::Keys(keys)
    }

    /// Adds everything `other` reads to what this reads.
    pub fn absorb(&mut self, other: SourceReads) {
        match (&mut *self, other) {
            (SourceReads::Keys(keys), SourceReads::Keys(more)) => keys.extend(more),
            (SourceReads::Everything, SourceReads::Keys(_) | SourceReads::Everything) => {}
            (SourceReads::Keys(_), SourceReads::Everything) => *self = SourceReads::Everything,
        }
    }

    /// Whether the top-level key `key` is read.
    #[must_use]
    pub fn reads(&self, key: &str) -> bool {
        match self {
            SourceReads::Keys(keys) => keys.contains(&SourceKey::new(key)),
            SourceReads::Everything => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        source_reads::{SourceKey, SourceReads},
        template::field_path::FieldPath,
    };
    use std::collections::BTreeSet;

    fn keys(names: &[&str]) -> SourceReads {
        SourceReads::Keys(names.iter().map(|name| SourceKey::new(name)).collect::<BTreeSet<SourceKey>>())
    }

    #[test]
    fn a_field_reference_reads_its_first_segment() {
        assert_eq!(SourceReads::of_field(&FieldPath::new("properties.name")), keys(&["properties"]));
        assert_eq!(SourceReads::of_field(&FieldPath::new("3")), keys(&["3"]));
    }

    #[test]
    fn the_whole_record_reference_reads_everything() {
        assert_eq!(SourceReads::of_field(&FieldPath::new("context")), SourceReads::Everything);
    }

    #[test]
    fn an_engine_template_reads_its_reported_variables_and_every_name_it_assigns() {
        assert_eq!(
            SourceReads::of_engine_template(["a", "b"], "{% if a %}{% set c = b %}{% endif %}{{ c }}"),
            keys(&["a", "b", "c"])
        );
    }

    #[test]
    fn an_engine_template_reading_the_record_or_an_engine_variable_reads_everything() {
        assert_eq!(SourceReads::of_engine_template(["this"], "{{ this['CO(GT)'] }}"), SourceReads::Everything);
        assert_eq!(SourceReads::of_engine_template([], "{{ __tera_context }}"), SourceReads::Everything);
    }

    #[test]
    fn absorbing_unions_keys_and_everything_wins() {
        let mut reads = SourceReads::nothing();
        reads.absorb(keys(&["a"]));
        reads.absorb(keys(&["b"]));
        assert_eq!(reads, keys(&["a", "b"]));
        assert!(reads.reads("a") && !reads.reads("c"));

        reads.absorb(SourceReads::Everything);
        assert_eq!(reads, SourceReads::Everything);
        assert!(reads.reads("c"));
    }

    #[test]
    fn nothing_reads_no_key() {
        assert!(!SourceReads::nothing().reads("a"));
    }
}
