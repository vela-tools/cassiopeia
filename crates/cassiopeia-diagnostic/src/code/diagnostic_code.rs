use crate::code::{
    broker_code::BrokerCode,
    catalog_code::CatalogCode,
    context_code::ContextCode,
    expander_code::ExpanderCode,
    extractor_code::ExtractorCode,
    geometry_code::GeometryCode,
    ingest_code::IngestCode,
    run_code::RunCode,
    schema_code::SchemaCode,
    transform_code::TransformCode,
};
use derive_more::{Display, From};
use strum::{EnumCount, IntoEnumIterator};

/// The stable, machine-readable name of why something failed.
///
/// A code is a subsystem plus a reason within it, and it renders as one kebab-case token
/// (`broker-entity-rejected`, `schema-nonconformant`), so the run summary's reason table, a log line,
/// and a support conversation all name a failure the same way. The sum is closed: a new reason is a
/// new variant of exactly one subsystem enum, never a free-form string.
#[derive(Clone, Copy, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
pub enum DiagnosticCode {
    /// A delivery to a Context Broker.
    #[display("broker-{_0}")]
    Broker(BrokerCode),
    /// A Smart Data Models catalog operation.
    #[display("catalog-{_0}")]
    Catalog(CatalogCode),
    /// Resolving or delivering the `@context`.
    #[display("context-{_0}")]
    Context(ContextCode),
    /// Expanding a record into fragments.
    #[display("expander-{_0}")]
    Expander(ExpanderCode),
    /// Extracting an entity's attribute values.
    #[display("extractor-{_0}")]
    Extractor(ExtractorCode),
    /// Admitting or converting a geometry.
    #[display("geometry-{_0}")]
    Geometry(GeometryCode),
    /// Bringing a source into the pipeline as records.
    #[display("ingest-{_0}")]
    Ingest(IngestCode),
    /// The run or one of its scheduled cycles.
    #[display("run-{_0}")]
    Run(RunCode),
    /// Checking an entity against its JSON Schema.
    #[display("schema-{_0}")]
    Schema(SchemaCode),
    /// Building an entity's NGSI-LD form.
    #[display("transform-{_0}")]
    Transform(TransformCode),
}

/// How many distinct codes the vocabulary holds.
pub const DIAGNOSTIC_CODE_COUNT: usize = BrokerCode::COUNT
    + CatalogCode::COUNT
    + ContextCode::COUNT
    + ExpanderCode::COUNT
    + ExtractorCode::COUNT
    + GeometryCode::COUNT
    + IngestCode::COUNT
    + RunCode::COUNT
    + SchemaCode::COUNT
    + TransformCode::COUNT;

impl DiagnosticCode {
    /// Every code the vocabulary defines, in subsystem order.
    pub fn all() -> impl Iterator<Item = DiagnosticCode> {
        BrokerCode::iter()
            .map(DiagnosticCode::Broker)
            .chain(CatalogCode::iter().map(DiagnosticCode::Catalog))
            .chain(ContextCode::iter().map(DiagnosticCode::Context))
            .chain(ExpanderCode::iter().map(DiagnosticCode::Expander))
            .chain(ExtractorCode::iter().map(DiagnosticCode::Extractor))
            .chain(GeometryCode::iter().map(DiagnosticCode::Geometry))
            .chain(IngestCode::iter().map(DiagnosticCode::Ingest))
            .chain(RunCode::iter().map(DiagnosticCode::Run))
            .chain(SchemaCode::iter().map(DiagnosticCode::Schema))
            .chain(TransformCode::iter().map(DiagnosticCode::Transform))
    }
}

#[cfg(test)]
mod tests {
    use crate::code::{
        broker_code::BrokerCode,
        diagnostic_code::{DIAGNOSTIC_CODE_COUNT, DiagnosticCode},
        schema_code::SchemaCode,
    };
    use std::collections::HashSet;

    #[test]
    fn every_code_renders_a_distinct_kebab_case_token() {
        let tokens: HashSet<String> = DiagnosticCode::all().map(|code| code.to_string()).collect();

        assert_eq!(tokens.len(), DIAGNOSTIC_CODE_COUNT);
        assert!(
            tokens
                .iter()
                .all(|token| token.chars().all(|character| character.is_ascii_lowercase() || character == '-'))
        );
    }

    #[test]
    fn the_documented_codes_render_exactly_as_published() {
        assert_eq!(DiagnosticCode::Broker(BrokerCode::EntityRejected).to_string(), "broker-entity-rejected");
        assert_eq!(DiagnosticCode::Schema(SchemaCode::Nonconformant).to_string(), "schema-nonconformant");
    }

    #[test]
    fn codes_order_by_subsystem_then_by_reason() {
        let mut codes = vec![
            DiagnosticCode::Schema(SchemaCode::Absent),
            DiagnosticCode::Broker(BrokerCode::BatchRejected),
            DiagnosticCode::Schema(SchemaCode::Nonconformant),
        ];
        codes.sort_unstable();

        assert_eq!(
            codes,
            vec![
                DiagnosticCode::Broker(BrokerCode::BatchRejected),
                DiagnosticCode::Schema(SchemaCode::Nonconformant),
                DiagnosticCode::Schema(SchemaCode::Absent),
            ]
        );
    }
}
