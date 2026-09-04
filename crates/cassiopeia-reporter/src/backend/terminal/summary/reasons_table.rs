use crate::backend::terminal::summary::{
    table_layout::{dim_header, header_row},
    thousands::thousands,
};
use cassiopeia_diagnostic::reason::Reason;
use cassiopeia_terminal_style::rendering::Rendering;
use tabular::{Row, Table};

/// Renders the reason table: what went wrong, how often, and one example of it.
///
/// This is what turns `errors 142` into something actionable. It says everything a trailing
/// "+ N more messages like this" line said, next to the code rather than orphaned at the bottom of
/// the run.
pub(crate) fn reasons_table(reasons: &[Reason], rendering: Rendering) -> Vec<String> {
    let mut table = Table::new("  {:<}  {:>}  {:<}");
    table.add_row(header_row(&["reason", "count", "example"]));
    for reason in reasons {
        table.add_row(
            Row::new()
                .with_cell(reason.code().to_string())
                .with_cell(thousands(reason.count()))
                .with_cell(reason.example()),
        );
    }
    dim_header(&table, rendering)
}

#[cfg(test)]
mod tests {
    use crate::backend::terminal::summary::reasons_table::reasons_table;
    use cassiopeia_diagnostic::{
        code::{broker_code::BrokerCode, diagnostic_code::DiagnosticCode, schema_code::SchemaCode},
        reason::Reason,
        severity::Severity,
    };
    use cassiopeia_terminal_style::rendering::Rendering;

    #[test]
    fn the_table_names_each_code_its_count_and_an_example() {
        let reasons = vec![
            Reason::new(
                Severity::Error,
                DiagnosticCode::Broker(BrokerCode::EntityRejected),
                142,
                "attribute 'dateObserved' is not a valid DateTime".into(),
            ),
            Reason::new(
                Severity::Warning,
                DiagnosticCode::Schema(SchemaCode::Nonconformant),
                18,
                "/temperature: required property missing".into(),
            ),
        ];

        assert_eq!(
            reasons_table(&reasons, Rendering::Plain),
            vec![
                "  reason                  count  example".to_owned(),
                "  broker-entity-rejected    142  attribute 'dateObserved' is not a valid DateTime".to_owned(),
                "  schema-nonconformant       18  /temperature: required property missing".to_owned(),
            ]
        );
    }
}
