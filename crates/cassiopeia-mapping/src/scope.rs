use crate::template::{CompiledTemplate, TemplateSource};
use serde::{Deserialize, Serialize};

/// The scope declaration on a mapping, before compilation.
///
/// NGSI-LD entities may carry one scope or several (ETSI GS CIM 009 v1.9.1 clause 4.18), so a
/// mapping may write either a single template or a list of them. Scope is an entity member rather
/// than part of the entity's identity: it can be updated, appended to, or deleted over the entity's
/// lifetime (clauses 5.6.2, 5.6.3, and 5.6.5), so it sits beside `dataModel` on the mapping instead
/// of inside `identity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scope {
    /// A single scope template.
    Single(TemplateSource),

    /// Several scope templates, each producing one scope.
    Multiple(Vec<TemplateSource>),
}

/// A `Scope` with its templates compiled, cached on the mapping after load.
#[derive(Debug, Clone)]
pub enum CompiledScope {
    /// A single compiled scope template.
    Single(CompiledScopeTemplate),

    /// Several compiled scope templates.
    Multiple(Vec<CompiledScopeTemplate>),
}

/// One compiled scope template, beside the template as the mapping document wrote it.
///
/// A record can be refused for what its scope template reads, such as a boolean field whose JSON
/// rendering is no scope the source meant; the refusal names the template as written, which the
/// compiled form no longer holds for a field reference or a concatenation.
#[derive(Debug, Clone)]
pub struct CompiledScopeTemplate {
    /// The template exactly as the mapping document wrote it.
    pub source: TemplateSource,
    /// The template's compiled form.
    pub compiled: CompiledTemplate,
}

#[cfg(test)]
mod tests {
    use crate::{scope::Scope, template::TemplateSource};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_bare_string_deserializes_as_a_single_scope() {
        let scope: Scope = serde_json::from_str(r#""/Ljubljana""#).unwrap();

        assert_eq!(scope, Scope::Single(TemplateSource::new("/Ljubljana")));
    }

    #[test]
    fn a_list_deserializes_as_multiple_scopes() {
        let scope: Scope = serde_json::from_str(r#"["/Ljubljana", "/Maribor"]"#).unwrap();

        assert_eq!(scope, Scope::Multiple(vec![TemplateSource::new("/Ljubljana"), TemplateSource::new("/Maribor")]));
    }

    #[test]
    fn a_single_scope_serializes_back_to_a_bare_string() {
        let scope = Scope::Single(TemplateSource::new("/Ljubljana"));

        assert_eq!(serde_json::to_string(&scope).unwrap(), r#""/Ljubljana""#);
    }
}
