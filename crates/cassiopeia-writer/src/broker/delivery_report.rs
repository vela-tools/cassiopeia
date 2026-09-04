use crate::{broker::broker_rejection::BrokerRejection, error::WriterError};
use cassiopeia_diagnostic::{
    cause::causes_of,
    code::{broker_code::BrokerCode, diagnostic_code::DiagnosticCode},
    context_field::{ContextField, minted_id},
    detail::Detail,
    diagnostic::Diagnostic,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use url::Url;

/// What every delivery diagnostic says regardless of what went wrong: where the request went and
/// which entities it carried.
pub struct DeliveryContext<'a> {
    /// The endpoint the request was addressed to.
    pub endpoint: &'a Url,
    /// The entities the request carried.
    pub entities: &'a [NgsiLdEntity],
}

/// Builds the diagnostic for one delivery failure.
///
/// The headline is written for a reader rather than taken from the error's `Display`, because the
/// same facts appear again as rows and a headline that repeats them reads twice as long for no gain.
/// The error is still passed in: its `source()` chain becomes the cause rows, which is how a
/// transport failure's real reason and an unreadable body's parse error survive to the report.
#[must_use]
pub fn delivery_diagnostic(
    severity: Severity,
    code: BrokerCode,
    headline: String,
    error: &WriterError,
    context: &DeliveryContext<'_>,
    extra: Vec<ContextField>,
) -> Diagnostic {
    let mut builder = DiagnosticBuilder::new(severity, DiagnosticCode::Broker(code), headline)
        .with_causes(causes_of(error))
        .with_context(ContextField::Endpoint(context.endpoint.clone()))
        .with_context(ContextField::BatchSize(u64::try_from(context.entities.len()).unwrap_or(u64::MAX)))
        .with_optional_context(entities_field(context.entities));
    for field in extra {
        builder = builder.with_context(field);
    }
    builder.build()
}

/// Builds a diagnostic for a failure that has no request behind it: a batch that could not be
/// serialized, a worker that died.
#[must_use]
pub fn writer_diagnostic(severity: Severity, code: BrokerCode, headline: String, error: &WriterError, extra: Vec<ContextField>) -> Diagnostic {
    let mut builder = DiagnosticBuilder::new(severity, DiagnosticCode::Broker(code), headline).with_causes(causes_of(error));
    for field in extra {
        builder = builder.with_context(field);
    }
    builder.build()
}

/// Names the entities a request covered: the first one, and how many followed it.
#[must_use]
pub fn entities_field(entities: &[NgsiLdEntity]) -> Option<ContextField> {
    let first = entities.first()?;
    Some(ContextField::Entities {
        first: minted_id(&first.id)?,
        additional: u64::try_from(entities.len().saturating_sub(1)).unwrap_or(u64::MAX),
    })
}

/// The fields a broker's own problem body contributes: what it called the problem, and what it said
/// about this occurrence.
#[must_use]
pub fn rejection_fields(rejection: &BrokerRejection) -> Vec<ContextField> {
    vec![
        ContextField::ProblemType(rejection.problem().problem_type.clone()),
        ContextField::Detail(Detail::new(rejection.text())),
    ]
}

#[cfg(test)]
mod tests {
    use crate::{
        broker::{
            broker_rejection::BrokerRejection,
            delivery_report::{DeliveryContext, delivery_diagnostic, entities_field, rejection_fields},
            problem_details::ProblemDetails,
        },
        error::WriterError,
    };
    use cassiopeia_diagnostic::{
        code::broker_code::BrokerCode,
        context_field::{ContextField, ContextFieldKind},
        severity::Severity,
    };
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use iri_rs::IriBuf;
    use url::Url;
    use urn_rs::Urn;

    fn entities(count: usize) -> Vec<NgsiLdEntity> {
        (0..count)
            .map(|index| {
                NgsiLdEntity::new(
                    format!("urn:ngsi-ld:AirQualityObserved:LJ-{index:03}").parse::<Urn>().unwrap(),
                    NameBuf::new("AirQualityObserved").unwrap(),
                )
            })
            .collect()
    }

    fn endpoint() -> Url {
        Url::parse("https://broker/ngsi-ld/v1/entityOperations/upsert").unwrap()
    }

    #[test]
    fn the_entities_field_names_the_first_and_counts_the_rest() {
        let field = entities_field(&entities(100)).expect("a field");

        assert_eq!(
            field,
            ContextField::Entities {
                first: IriBuf::new("urn:ngsi-ld:AirQualityObserved:LJ-000".to_owned()).unwrap(),
                additional: 99,
            }
        );
    }

    #[test]
    fn an_empty_batch_contributes_no_entities_field() {
        assert!(entities_field(&[]).is_none());
    }

    #[test]
    fn a_delivery_diagnostic_keeps_the_errors_chain_as_causes() {
        let error = WriterError::BrokerMultiStatusUnreadable {
            source: serde_json::from_str::<serde_json::Value>("not json").unwrap_err(),
            count: 100,
        };
        let batch = entities(100);

        let diagnostic = delivery_diagnostic(
            Severity::Error,
            BrokerCode::BatchUnreadable,
            "Broker 207 body could not be read".to_owned(),
            &error,
            &DeliveryContext {
                endpoint: &endpoint(),
                entities: &batch,
            },
            Vec::new(),
        );

        assert_eq!(diagnostic.causes().len(), 1);
        assert!(diagnostic.incidental().iter().any(|field| field.kind() == ContextFieldKind::Entities));
    }

    #[test]
    fn the_problem_body_contributes_its_type_and_its_explanation() {
        let problem: ProblemDetails = serde_json::from_str(r#"{"type":"urn:ex:bad","detail":"not a valid DateTime"}"#).unwrap();

        let fields = rejection_fields(&BrokerRejection::new(problem));

        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].kind(), ContextFieldKind::ProblemType);
        assert_eq!(fields[1].kind(), ContextFieldKind::Detail);
    }
}
