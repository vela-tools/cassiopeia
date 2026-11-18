use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
use derive_more::Display;
use langtag::LangTagBuf;
use std::num::NonZeroUsize;

/// Where one entry stands among an attribute's `instances`, counted from 1 as a reader counts them.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub struct InstancePosition(NonZeroUsize);

impl InstancePosition {
    /// The position of the entry at `index` of the `instances` list, counted from 0.
    #[must_use]
    pub const fn of_index(index: usize) -> InstancePosition {
        InstancePosition(NonZeroUsize::MIN.saturating_add(index))
    }
}

/// Which entity of a mapping document a declaration describes.
///
/// A `syntheticEntity` names its attributes from its own top level, so an attribute of it is told
/// apart from one of the document's by the attribute that declares the synthetic entity.
#[derive(Clone, Debug, Display, Eq, PartialEq)]
pub enum DeclaringEntity {
    /// The entity the document itself describes.
    #[display("")]
    Main,
    /// The `syntheticEntity` that the document's top-level attribute of this name declares.
    #[display(" in the syntheticEntity of `{_0}`")]
    Synthetic(NameBuf),
}

/// One declaration within an entity of a mapping document.
///
/// A sub-attribute of an instance is named as a sub-attribute of the attribute, since the instances
/// of an attribute share its sub-attributes, and a declaration nested beneath a `languageMap` entry
/// is named under the attribute that holds the entry.
#[derive(Clone, Debug, Display, Eq, PartialEq)]
pub enum Declaration {
    /// An attribute, a sub-attribute under `properties` or a member under `mappings`, named by its
    /// path from the top-level attribute, such as `temperature.observedAt` or `address[street]`.
    #[display("`{_0}`")]
    Attribute(AttributePath),
    /// One entry of the `instances` of the attribute at `attribute`.
    #[display("Instance {position} of `{attribute}`")]
    Instance {
        /// The attribute declaring the instances.
        attribute: AttributePath,
        /// Where the entry stands among them.
        position: InstancePosition,
    },
    /// The `languageMap` entry for `language` of the attribute at `attribute`.
    #[display("The languageMap entry `{language}` of `{attribute}`")]
    LanguageEntry {
        /// The attribute declaring the `languageMap`.
        attribute: AttributePath,
        /// The language tag the entry is declared under.
        language: LangTagBuf,
    },
    /// The `target` of the relationship at the path.
    #[display("The target of `{_0}`")]
    Target(AttributePath),
}

impl Declaration {
    /// The attribute, sub-attribute or member the declaration is, or belongs to.
    #[must_use]
    pub const fn attribute(&self) -> &AttributePath {
        match self {
            Declaration::Attribute(attribute)
            | Declaration::Target(attribute)
            | Declaration::Instance { attribute, .. }
            | Declaration::LanguageEntry { attribute, .. } => attribute,
        }
    }
}

/// Where in a mapping document a declaration sits: which entity it describes, and which declaration
/// of that entity it is.
///
/// It renders as the subject of a sentence, such as ``Instance 2 of `forecast` `` or
/// ``The target of `owner` in the syntheticEntity of `site` ``.
#[derive(Clone, Debug, Display, Eq, PartialEq)]
#[display("{declaration}{entity}")]
pub struct DeclarationSite {
    /// The entity the declaration describes.
    entity: DeclaringEntity,
    /// The declaration within that entity.
    declaration: Declaration,
}

impl DeclarationSite {
    /// The site of `declaration` within `entity`.
    #[must_use]
    pub const fn new(entity: DeclaringEntity, declaration: Declaration) -> DeclarationSite {
        DeclarationSite { entity, declaration }
    }

    /// The document's own top-level attribute the declaration is found beneath: the one whose
    /// `syntheticEntity` holds it, or else the one its path starts at.
    #[must_use]
    pub const fn top_level_attribute(&self) -> &NameBuf {
        match &self.entity {
            DeclaringEntity::Main => self.declaration.attribute().root(),
            DeclaringEntity::Synthetic(attribute) => attribute,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::declaration_site::{Declaration, DeclarationSite, DeclaringEntity, InstancePosition};
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
    use langtag::LangTagBuf;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn main(declaration: Declaration) -> DeclarationSite {
        DeclarationSite::new(DeclaringEntity::Main, declaration)
    }

    #[test]
    fn an_instance_is_counted_from_one() {
        assert_eq!(InstancePosition::of_index(0).to_string(), "1");
        assert_eq!(InstancePosition::of_index(4).to_string(), "5");
    }

    #[test]
    fn each_declaration_of_the_main_entity_renders_as_the_subject_of_a_sentence() {
        let forecast = AttributePath::top_level(name("forecast"));

        assert_eq!(
            main(Declaration::Attribute(AttributePath::new(
                name("temperature"),
                vec![name("observedAt")],
                Vec::new()
            )))
            .to_string(),
            "`temperature.observedAt`"
        );
        assert_eq!(
            main(Declaration::Attribute(AttributePath::new(name("address"), Vec::new(), vec![name("street")]))).to_string(),
            "`address[street]`"
        );
        assert_eq!(
            main(Declaration::Instance {
                attribute: forecast,
                position: InstancePosition::of_index(1),
            })
            .to_string(),
            "Instance 2 of `forecast`"
        );
        assert_eq!(
            main(Declaration::LanguageEntry {
                attribute: AttributePath::top_level(name("label")),
                language: LangTagBuf::new("pt-BR".to_owned()).unwrap(),
            })
            .to_string(),
            "The languageMap entry `pt-BR` of `label`"
        );
        assert_eq!(
            main(Declaration::Target(AttributePath::top_level(name("refBuilding")))).to_string(),
            "The target of `refBuilding`"
        );
    }

    #[test]
    fn a_declaration_of_a_synthetic_entity_names_the_attribute_declaring_it() {
        let site = DeclarationSite::new(
            DeclaringEntity::Synthetic(name("owner")),
            Declaration::Attribute(AttributePath::top_level(name("name"))),
        );

        assert_eq!(site.to_string(), "`name` in the syntheticEntity of `owner`");
        assert_eq!(site.top_level_attribute(), &name("owner"));
    }

    #[test]
    fn a_declaration_of_the_main_entity_is_found_beneath_the_root_of_its_path() {
        let site = main(Declaration::Target(AttributePath::new(
            name("hasLeadActor"),
            vec![name("playsCharacter")],
            Vec::new(),
        )));

        assert_eq!(site.top_level_attribute(), &name("hasLeadActor"));
    }
}
