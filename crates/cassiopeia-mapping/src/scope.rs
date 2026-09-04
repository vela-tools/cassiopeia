use crate::template::{CompiledTemplate, TemplateSource};
use serde::{Deserialize, Serialize};

/// The scope declaration on a mapping's identity, before compilation.
///
/// NGSI-LD entities may carry one scope or several (ETSI GS CIM 009 v1.9.1 clause 4.18), so a
/// mapping may write either a single template or a list of them.
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
    Single(CompiledTemplate),

    /// Several compiled scope templates.
    Multiple(Vec<CompiledTemplate>),
}

#[cfg(test)]
mod tests {
    use crate::{scope::Scope, template::TemplateSource};

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
