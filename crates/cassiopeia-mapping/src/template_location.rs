use crate::template_site::TemplateSite;
use derive_more::Display;
use std::{path::Path, sync::Arc};

/// Where a template expression is declared: the mapping document holding it and the declaration
/// within that document.
///
/// Every template failure, whether the template cannot be compiled when the mapping is loaded or
/// cannot be resolved against one record, is reported against its location, so the author is
/// pointed at the document and the key to fix rather than at the expression text alone. Its
/// rendering is the one phrasing of "where" those reports share.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
#[display("{site} template in the mapping document at '{}'", document.display())]
pub struct TemplateLocation {
    /// The mapping document declaring the template, shared by every template the document declares.
    pub document: Arc<Path>,
    /// The declaration within the document.
    pub site: TemplateSite,
}

impl TemplateLocation {
    /// Locates a template at `site` in the mapping document at `document`.
    #[must_use]
    pub const fn new(document: Arc<Path>, site: TemplateSite) -> TemplateLocation {
        TemplateLocation { document, site }
    }
}

#[cfg(test)]
mod tests {
    use crate::{template_location::TemplateLocation, template_site::TemplateSite};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use std::{path::Path, sync::Arc};

    #[test]
    fn a_location_names_the_site_and_the_document() {
        let location = TemplateLocation::new(
            Arc::from(Path::new("/maps/sensor.json5")),
            TemplateSite::Attribute(NameBuf::new("temperature").unwrap()),
        );

        assert_eq!(
            location.to_string(),
            "attribute `temperature` template in the mapping document at '/maps/sensor.json5'"
        );
    }

    #[test]
    fn an_identity_location_names_the_identity_key() {
        let location = TemplateLocation::new(Arc::from(Path::new("sensor.json5")), TemplateSite::EntityName);

        assert_eq!(location.to_string(), "identity.entityName template in the mapping document at 'sensor.json5'");
    }
}
