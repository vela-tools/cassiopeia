use derive_more::Display;

/// The name a complex template is registered under in the Tera engine.
///
/// A newtype rather than a bare `String` so the digest-derived registration key cannot be confused
/// with a field path or literal text; the name is a digest of the template source, so the same
/// expression always registers under the same name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
#[display("{_0}")]
pub struct TemplateName(String);

impl TemplateName {
    /// Derives the registration name for a template source expression.
    #[must_use]
    pub fn for_source(source: &str) -> TemplateName {
        TemplateName(format!("tpl_{:x}", md5::compute(source)))
    }

    /// The name as a string slice, for handing to the Tera engine.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::template::template_name::TemplateName;

    #[test]
    fn the_same_source_yields_the_same_name() {
        assert_eq!(TemplateName::for_source("{{ a | upper }}"), TemplateName::for_source("{{ a | upper }}"));
    }

    #[test]
    fn different_sources_yield_different_names() {
        assert_ne!(TemplateName::for_source("{{ a }}"), TemplateName::for_source("{{ b }}"));
    }

    #[test]
    fn the_name_is_prefixed_so_it_is_a_valid_identifier() {
        assert!(TemplateName::for_source("{{ a }}").as_str().starts_with("tpl_"));
    }
}
