use crate::context_field::{ContextField, ContextFieldKind};

/// Whether a context field takes part in a diagnostic's identity.
///
/// The split is what keeps the terminal quiet. A hundred entities rejected for one reason must
/// collapse into one line with a count, which they can only do if the entity ids they differ by stay
/// out of the key. Anything that names *why* something failed is defining; anything that names
/// *which occurrence* is incidental.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FieldRole {
    /// The field distinguishes one failure from another, so it belongs to the identity.
    Defining,
    /// The field describes one occurrence, so it is carried alongside the identity.
    Incidental,
}

impl ContextField {
    /// Whether this field defines the failure or merely describes one occurrence of it.
    #[must_use]
    pub fn role(&self) -> FieldRole {
        match self.kind() {
            ContextFieldKind::HttpStatus
            | ContextFieldKind::Endpoint
            | ContextFieldKind::ProblemType
            | ContextFieldKind::EntityType
            | ContextFieldKind::Attribute
            | ContextFieldKind::InstancePath
            | ContextFieldKind::SchemaPath
            | ContextFieldKind::Keyword
            | ContextFieldKind::Detail => FieldRole::Defining,
            ContextFieldKind::Entities
            | ContextFieldKind::SourcePath
            | ContextFieldKind::Attempt
            | ContextFieldKind::PayloadBytes
            | ContextFieldKind::BodyEcho
            | ContextFieldKind::RegistrationId
            | ContextFieldKind::BatchSize => FieldRole::Incidental,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_field::{ContextField, ContextFieldKind},
        detail::Detail,
        field_role::FieldRole,
        json_pointer::JsonPointer,
        schema_keyword::SchemaKeyword,
    };
    use cassiopeia_common::captured_body::CapturedBody;
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use http::StatusCode;
    use iri_rs::IriBuf;
    use std::{collections::HashSet, path::PathBuf};
    use strum::IntoEnumIterator;
    use url::Url;

    /// One field of every kind, so a role assertion can be made over the whole vocabulary.
    fn every_field() -> Vec<ContextField> {
        vec![
            ContextField::HttpStatus(StatusCode::BAD_REQUEST),
            ContextField::Endpoint(Url::parse("https://broker/ngsi-ld/v1/entityOperations/upsert").unwrap()),
            ContextField::ProblemType("about:blank".parse().unwrap()),
            ContextField::EntityType(NameBuf::new("AirQualityObserved").unwrap()),
            ContextField::Attribute(NameBuf::new("dateObserved").unwrap()),
            ContextField::InstancePath(JsonPointer::new("/dateObserved")),
            ContextField::SchemaPath(JsonPointer::new("/properties/dateObserved/format")),
            ContextField::Keyword(SchemaKeyword::Format),
            ContextField::Detail(Detail::new("not a valid DateTime")),
            ContextField::Entities {
                first: IriBuf::new("urn:ngsi-ld:A:1".to_owned()).unwrap(),
                additional: 99,
            },
            ContextField::SourcePath(PathBuf::from("/data/air.csv")),
            ContextField::Attempt { attempt: 2, limit: 3 },
            ContextField::PayloadBytes(4096),
            ContextField::BodyEcho(CapturedBody::capped(b"<html>".to_vec(), 2048)),
            ContextField::RegistrationId(IriBuf::new("urn:ngsi-ld:ContextSourceRegistration:1".to_owned()).unwrap()),
            ContextField::BatchSize(100),
        ]
    }

    #[test]
    fn the_sample_covers_every_field_kind() {
        let covered: HashSet<ContextFieldKind> = every_field().iter().map(ContextField::kind).collect();

        assert_eq!(covered, ContextFieldKind::iter().collect::<HashSet<_>>());
    }

    #[test]
    fn exactly_the_occurrence_describing_fields_are_incidental() {
        let incidental: HashSet<ContextFieldKind> = every_field()
            .iter()
            .filter(|field| field.role() == FieldRole::Incidental)
            .map(ContextField::kind)
            .collect();

        assert_eq!(
            incidental,
            HashSet::from([
                ContextFieldKind::Entities,
                ContextFieldKind::SourcePath,
                ContextFieldKind::Attempt,
                ContextFieldKind::PayloadBytes,
                ContextFieldKind::BodyEcho,
                ContextFieldKind::RegistrationId,
                ContextFieldKind::BatchSize,
            ])
        );
    }

    #[test]
    fn every_other_field_is_defining() {
        let defining = every_field().iter().filter(|field| field.role() == FieldRole::Defining).count();

        assert_eq!(defining, every_field().len() - 7);
    }
}
