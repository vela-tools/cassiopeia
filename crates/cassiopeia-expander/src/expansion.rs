use crate::{error::ExpanderError, expansion_losses::ExpansionLosses, urn::error::UrnError};
use cassiopeia_ir::{relationship_path::RelationshipPath, source_position::SourcePosition};
use cassiopeia_mapping::template::template_input::TemplateInput;
use urn_rs::Urn;

/// The record one expansion is working through: its entity, its data with the run variables it is
/// read with, its place in the source, and where its losses go.
///
/// Every relationship the record declares is minted against the same four, so they travel
/// together rather than as four parameters threaded through each minting step.
pub(crate) struct Expansion<'a> {
    /// The entity the record expands into, which every link it loses is recorded against.
    pub(crate) entity: &'a Urn,
    /// The record's data and run variables, read by every relationship source and synthetic identity.
    pub(crate) input: TemplateInput<'a>,
    /// The record's place in the source; each fragment it expands into takes the place of the
    /// record's next fragment.
    pub(crate) position: SourcePosition,
    /// Where the record's losses are recorded.
    pub(crate) losses: &'a ExpansionLosses,
}

impl Expansion<'_> {
    /// Keeps what minting one link yielded, or nothing when the link names no target.
    ///
    /// A source that resolves to no identifier names no target, exactly as an absent foreign key
    /// does. A template that fails to render is the same gap in the data, so it too costs only the
    /// link, not the record: the failure is recorded under the relationship's `path`, built only
    /// then, so the run can still name it. Every other failure is a fault in the mapping and fails
    /// the record.
    pub(crate) fn keep<T>(&self, minted: Result<T, UrnError>, path: impl FnOnce() -> RelationshipPath) -> Result<Option<T>, ExpanderError> {
        match minted {
            Ok(minted) => Ok(Some(minted)),
            Err(UrnError::GeneratedIdEmpty { .. }) => Ok(None),
            Err(UrnError::Template(error)) => {
                self.losses.links.record(&path(), self.entity, error);
                Ok(None)
            }
            Err(other @ (UrnError::BuildUrn { .. } | UrnError::RelationshipMissingSource | UrnError::NoRelationshipTarget)) => Err(other.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::ExpanderError, expansion::Expansion, expansion_losses::ExpansionLosses, urn::error::UrnError};
    use cassiopeia_batch_sink::batch_sink::BatchSink;
    use cassiopeia_ir::{relationship_path::RelationshipPath, source_position::SourcePosition};
    use cassiopeia_mapping::{
        template::{
            TemplateSource,
            error::{ResolutionFailure, TemplateError},
            run_variables::RunVariables,
            template_input::TemplateInput,
        },
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::{path::Path, sync::Arc};
    use urn_rs::Urn;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).unwrap()
    }

    fn entity() -> Urn {
        "urn:ngsi-ld:Building:A".parse().unwrap()
    }

    fn owner() -> RelationshipPath {
        RelationshipPath::flat(name("owner"))
    }

    /// A relationship source that failed to render for the record.
    fn failure() -> TemplateError {
        TemplateError {
            location: TemplateLocation::new(
                Arc::from(Path::new("building.json5")),
                TemplateSite::Attribute(AttributePath::top_level(name("owner"))),
            ),
            failure: Box::new(ResolutionFailure::Decode {
                template: TemplateSource::new("{{ owners | split(pat=' ') }}"),
                source: serde_json::from_str::<Value>("[").unwrap_err(),
            }),
        }
    }

    /// No link lost.
    const NO_LOSS: [(String, String); 0] = [];

    /// What keeping one minted link left behind.
    struct Kept {
        /// What the link kept, or why the record failed.
        kept: Result<Option<u8>, ExpanderError>,
        /// Every lost link, as its dotted path and the entity that lost it.
        lost: Vec<(String, String)>,
    }

    /// Keeps `minted` for a record of the `Building:A` entity.
    fn keep(minted: Result<u8, UrnError>) -> Kept {
        let entity = entity();
        let record = json!({});
        let variables = RunVariables::none();
        let losses = ExpansionLosses::new();
        let expansion = Expansion {
            entity: &entity,
            input: TemplateInput::new(&record, &variables),
            position: SourcePosition::first_of(0),
            losses: &losses,
        };

        let kept = expansion.keep(minted, owner);
        let lost = BatchSink::from(losses.links)
            .into_entries()
            .map(|(path, lost)| (path.to_string(), lost.entity.to_string()))
            .collect();
        Kept { kept, lost }
    }

    #[test]
    fn a_minted_link_is_kept() {
        let Kept { kept, lost } = keep(Ok(7));

        assert_eq!(kept.unwrap(), Some(7));
        assert_eq!(lost, NO_LOSS);
    }

    #[test]
    fn a_link_whose_source_names_no_target_is_dropped_without_a_loss() {
        let Kept { kept, lost } = keep(Err(UrnError::GeneratedIdEmpty {
            target_entity_type: name("Owner"),
        }));

        assert_eq!(kept.unwrap(), None);
        assert_eq!(lost, NO_LOSS);
    }

    #[test]
    fn a_link_whose_source_fails_to_render_is_dropped_and_recorded_under_its_path() {
        let Kept { kept, lost } = keep(Err(UrnError::Template(failure())));

        assert_eq!(kept.unwrap(), None);
        assert_eq!(lost, [("owner".to_owned(), "urn:ngsi-ld:Building:A".to_owned())]);
    }

    #[test]
    fn a_relationship_without_a_source_fails_the_record() {
        let Kept { kept, lost } = keep(Err(UrnError::RelationshipMissingSource));

        assert!(matches!(kept, Err(ExpanderError::Urn(UrnError::RelationshipMissingSource))), "{kept:?}");
        assert_eq!(lost, NO_LOSS);
    }
}
