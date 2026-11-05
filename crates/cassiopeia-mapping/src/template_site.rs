use cassiopeia_ngsi_ld::entity::attribute_path::AttributePath;
use derive_more::Display;

/// Where in a mapping document a template expression is declared.
///
/// A template that cannot be compiled is reported against the declaration holding it, so the author
/// can find it without searching the document for the expression text.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum TemplateSite {
    /// The `identity.entityName` template.
    #[display("identity.entityName")]
    EntityName,

    /// One of the `scope` templates.
    #[display("scope")]
    Scope,

    /// The entity `expiresAt` template.
    #[display("expiresAt")]
    ExpiresAt,

    /// The `observedAt` template the mapping reads its temporality from, compiled when the document
    /// is loaded.
    #[display("observedAt")]
    ObservedAt,

    /// The `source` of the attribute at the path, which runs from the top-level attribute down, so a
    /// sub-attribute two attributes both declare, such as `accuracy`, says which one it qualifies. A
    /// per-language entry or an instance is reported under the attribute that declares it, since
    /// neither carries an attribute name of its own.
    #[display("attribute `{_0}`")]
    Attribute(AttributePath),
}

#[cfg(test)]
mod tests {
    use crate::template_site::TemplateSite;
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
    use pretty_assertions::assert_eq;

    #[test]
    fn each_site_renders_the_document_key_it_stands_for() {
        assert_eq!(TemplateSite::EntityName.to_string(), "identity.entityName");
        assert_eq!(TemplateSite::Scope.to_string(), "scope");
        assert_eq!(TemplateSite::ExpiresAt.to_string(), "expiresAt");
        assert_eq!(TemplateSite::ObservedAt.to_string(), "observedAt");
        assert_eq!(
            TemplateSite::Attribute(AttributePath::top_level(NameBuf::new("temperature").unwrap())).to_string(),
            "attribute `temperature`"
        );
    }

    #[test]
    fn a_sub_attribute_site_renders_its_path_from_the_top_level_attribute() {
        let path = AttributePath::new(NameBuf::new("hasLeadActor").unwrap(), vec![NameBuf::new("castSize").unwrap()], Vec::new());

        assert_eq!(TemplateSite::Attribute(path).to_string(), "attribute `hasLeadActor.castSize`");
    }
}
