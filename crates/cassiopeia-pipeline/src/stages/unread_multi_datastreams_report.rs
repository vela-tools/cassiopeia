use cassiopeia_collector::{collection_warnings::CollectionWarnings, sensorthings::multi_datastreams_unread::MultiDatastreamsUnread};
use cassiopeia_common::{input::Input, masked_url::MaskedUrl};
use cassiopeia_diagnostic::{
    code::{collect_code::CollectCode, diagnostic_code::DiagnosticCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::DiagnosticSink;

/// The one-line summary: the server, and the setting that selects its `MultiDatastreams`.
fn headline(unread: &MultiDatastreamsUnread) -> String {
    format!(
        "`{}` holds MultiDatastreams, which this input reads none of; set `multiDatastreams` to `true`, or to an OData expression that selects some, to read their readings",
        unread.root()
    )
}

/// Names every server that holds `MultiDatastreams` its input reads none of in one warning each, and
/// returns how many warnings that was.
///
/// Each warning names its server as the source, so the servers of a run with several are told apart.
pub(crate) fn report_unread_multi_datastreams(sink: &dyn DiagnosticSink, warnings: &CollectionWarnings) -> u64 {
    warnings.unread_multi_datastreams().iter().fold(0, |reported, unread| {
        sink.report(
            &DiagnosticBuilder::new(
                Severity::Warning,
                DiagnosticCode::Collect(CollectCode::MultiDatastreamsUnread),
                headline(unread),
            )
            // The diagnostic owns its source, while the warnings keep the root they name.
            .with_context(ContextField::Source(Input::Remote(MaskedUrl::plain(unread.root().url().clone()))))
            .build(),
        );
        reported + 1
    })
}

#[cfg(test)]
mod tests {
    use crate::{stages::unread_multi_datastreams_report::report_unread_multi_datastreams, test_reporter::RecordingReporter};
    use cassiopeia_collector::{collection_warnings::CollectionWarnings, sensorthings::multi_datastreams_unread::MultiDatastreamsUnread};
    use cassiopeia_common::{input::Input, masked_url::MaskedUrl};
    use cassiopeia_diagnostic::{
        code::{collect_code::CollectCode, diagnostic_code::DiagnosticCode},
        context_field::ContextField,
    };
    use cassiopeia_sensorthings::service_root::ServiceRoot;
    use pretty_assertions::assert_eq;
    use url::Url;

    #[test]
    fn a_collection_that_left_nothing_unread_says_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_unread_multi_datastreams(&sink, &CollectionWarnings::default()), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn unread_multi_datastreams_are_one_warning_naming_the_server_and_the_setting() {
        let sink = RecordingReporter::new();
        let root = ServiceRoot::try_from(Url::parse("https://h.example/v1.1").unwrap()).unwrap();
        let mut warnings = CollectionWarnings::default();
        warnings.multi_datastreams_unread(MultiDatastreamsUnread::new(root));

        let total = report_unread_multi_datastreams(&sink, &warnings);

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported[0].code, DiagnosticCode::Collect(CollectCode::MultiDatastreamsUnread));
        assert_eq!(reported[0].code.to_string(), "collect-multi-datastreams-unread");
        assert_eq!(
            reported[0].headline,
            "`https://h.example/v1.1` holds MultiDatastreams, which this input reads none of; set `multiDatastreams` to `true`, or to an OData expression that selects some, to read their readings"
        );
        assert_eq!(
            reported[0].defining,
            [ContextField::Source(Input::Remote(MaskedUrl::plain(
                Url::parse("https://h.example/v1.1").unwrap()
            )))]
        );
    }
}
