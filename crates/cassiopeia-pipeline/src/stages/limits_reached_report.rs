use cassiopeia_collector::{collection_warnings::CollectionWarnings, sensorthings::limit_reached::LimitReached};
use cassiopeia_common::{input::Input, masked_url::MaskedUrl};
use cassiopeia_diagnostic::{
    code::{collect_code::CollectCode, diagnostic_code::DiagnosticCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::DiagnosticSink;

/// The one-line summary: the server, the limit and its value, and the next link not followed, so a
/// reader can raise the limit or narrow the read.
fn headline(limit: &LimitReached) -> String {
    format!(
        "In `{}`, {} stopped the read before the server's last page, so this cycle carries on with what it collected; the next link `{}` was not followed",
        limit.root(),
        limit.limit(),
        limit.next()
    )
}

/// Names every collection limit that stopped a read in one warning each, and returns how many
/// warnings that was.
///
/// Each warning names its server as the source, so a limit reached on one server of a run with several
/// is told apart from the rest.
pub(crate) fn report_limits_reached(sink: &dyn DiagnosticSink, warnings: &CollectionWarnings) -> u64 {
    warnings.limits_reached().iter().fold(0, |reported, limit| {
        sink.report(
            &DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Collect(CollectCode::LimitReached), headline(limit))
                // The diagnostic owns its source, while the warnings keep the root they name.
                .with_context(ContextField::Source(Input::Remote(MaskedUrl::plain(limit.root().url().clone()))))
                .build(),
        );
        reported + 1
    })
}

#[cfg(test)]
mod tests {
    use crate::{stages::limits_reached_report::report_limits_reached, test_reporter::RecordingReporter};
    use cassiopeia_collector::{
        collection_warnings::CollectionWarnings,
        sensorthings::limit_reached::{CollectLimit, LimitReached},
    };
    use cassiopeia_common::{input::Input, masked_url::MaskedUrl};
    use cassiopeia_diagnostic::{
        code::{collect_code::CollectCode, diagnostic_code::DiagnosticCode},
        context_field::ContextField,
    };
    use cassiopeia_sensorthings::{service_root::ServiceRoot, settings::collect_limits::PageLimit};
    use pretty_assertions::assert_eq;
    use url::Url;

    #[test]
    fn a_collection_without_limits_reached_says_nothing_and_counts_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_limits_reached(&sink, &CollectionWarnings::default()), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn a_page_limit_is_one_warning_naming_the_limit_and_the_link_not_followed() {
        let sink = RecordingReporter::new();
        let root = Url::parse("https://h.example/v1.1").unwrap();
        let limit = LimitReached::new(
            ServiceRoot::try_from(root.clone()).unwrap(),
            CollectLimit::Pages(PageLimit::try_from(2).unwrap()),
            Url::parse("https://h.example/v1.1/Datastreams?$skip=2000").unwrap(),
        );
        let mut warnings = CollectionWarnings::default();
        warnings.limit_reached(limit);

        let total = report_limits_reached(&sink, &warnings);

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Collect(CollectCode::LimitReached));
        assert_eq!(reported[0].code.to_string(), "collect-limit-reached");
        assert_eq!(
            reported[0].headline,
            "In `https://h.example/v1.1`, `limits.pages: 2` stopped the read before the server's last page, so this cycle carries on with what it collected; the next link `https://h.example/v1.1/Datastreams?$skip=2000` was not followed"
        );
        assert_eq!(reported[0].defining, [ContextField::Source(Input::Remote(MaskedUrl::plain(root)))]);
    }
}
