use crate::{scope::Scope, template::TemplateSource};

/// The optional entity members a mapping declares beside its `identity`: the `scope` set (ETSI GS
/// CIM 009 v1.9.1 clause 4.18) and the entity `expiresAt` instant (clause 4.5.1).
///
/// Both are reserved entity members resolved per record and merged when several records contribute
/// to one entity, neither belongs inside the identity, and neither has a default, which is why the
/// programmatic [`Mapping`](crate::mapping::Mapping) constructor takes them as one argument rather
/// than as two positions that every caller fills with `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntityMembers {
    /// The scope declaration, a single template or a list of them.
    pub scope: Option<Scope>,
    /// The entity `expiresAt` template, named `expiresAt` in the document.
    pub expires_at: Option<TemplateSource>,
}

#[cfg(test)]
mod tests {
    use crate::{entity_members::EntityMembers, scope::Scope, template::TemplateSource};
    use pretty_assertions::assert_eq;

    #[test]
    fn an_empty_declaration_defaults_to_no_optional_members() {
        assert_eq!(EntityMembers::default(), EntityMembers { scope: None, expires_at: None });
    }

    #[test]
    fn a_declaration_carries_both_members_independently() {
        let members = EntityMembers {
            scope: Some(Scope::Single(TemplateSource::new("/test"))),
            expires_at: Some(TemplateSource::new("{{ until }}")),
        };

        assert_eq!(members.scope, Some(Scope::Single(TemplateSource::new("/test"))));
        assert_eq!(members.expires_at, Some(TemplateSource::new("{{ until }}")));
    }
}
