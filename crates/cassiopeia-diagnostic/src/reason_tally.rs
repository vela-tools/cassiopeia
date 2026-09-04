use crate::{
    code::diagnostic_code::DiagnosticCode,
    diagnostic::Diagnostic,
    diagnostic_identity::DiagnosticIdentity,
    reason::{Reason, Sighting},
    severity::Severity,
};
use dashmap::DashMap;
use std::{cmp::Reverse, collections::HashMap};

/// What one identity has accounted for so far.
struct Tallied {
    /// How many occurrences were recorded under this identity.
    occurrences: u64,
    /// The explanation the first occurrence carried.
    example: Box<str>,
}

/// The run's live tally of distinct failures, keyed by [`DiagnosticIdentity`].
///
/// This is one map at two grains: the deduplicating middleware asks it whether a diagnostic is new
/// (so it renders once), and the run summary asks it to fold the same entries by code into the
/// reason table. A second map would be the same data twice.
///
/// It is a [`DashMap`] because failures are recorded from every stage's worker threads at once and a
/// lock around a `HashMap` would serialise them on the one path a struggling run takes constantly.
#[derive(Default)]
pub struct ReasonTally {
    /// Every distinct failure seen since the last drain.
    seen: DashMap<DiagnosticIdentity, Tallied>,
}

impl ReasonTally {
    /// Opens an empty tally.
    #[must_use]
    pub fn new() -> ReasonTally {
        ReasonTally::default()
    }

    /// Records one diagnostic, saying whether its identity had been seen before.
    ///
    /// A diagnostic that already stands for several occurrences contributes all of them, so a
    /// warned-nonconformance summary reporting 1 482 entities adds 1 482 to its code, not one.
    #[must_use]
    pub fn record(&self, diagnostic: &Diagnostic) -> Sighting {
        let occurrences = diagnostic.occurrences().get();
        if let Some(mut tallied) = self.seen.get_mut(diagnostic.identity()) {
            tallied.occurrences = tallied.occurrences.saturating_add(occurrences);
            return Sighting::Repeat;
        }

        // The map key must own the identity; cloning it once per distinct failure is unavoidable and
        // is paid only when something new goes wrong.
        self.seen.insert(
            diagnostic.identity().clone(),
            Tallied {
                occurrences,
                example: diagnostic.explanation().into(),
            },
        );
        Sighting::First
    }

    /// Whether nothing has been recorded since the last drain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }

    /// Folds every recorded identity into one row per code and empties the tally.
    ///
    /// Draining is what lets a scheduled run's second cycle report afresh: without it the identities
    /// of the first cycle would suppress every repeat for the rest of the process.
    ///
    /// Rows are ordered by severity, then by descending count, then by code, so the worst and
    /// loudest reason leads. The example for a code is the lexicographically smallest of its
    /// identities' explanations, which makes the table reproducible whatever order a parallel run
    /// happened to record them in.
    #[must_use]
    pub fn drain(&self) -> Vec<Reason> {
        let mut folded: HashMap<(Severity, DiagnosticCode), (u64, Box<str>)> = HashMap::new();
        for entry in &self.seen {
            let key = (entry.key().severity(), entry.key().code());
            let occurrences = entry.value().occurrences;
            let example = entry.value().example.clone();
            folded
                .entry(key)
                .and_modify(|(count, kept)| {
                    *count = count.saturating_add(occurrences);
                    if example < *kept {
                        kept.clone_from(&example);
                    }
                })
                .or_insert((occurrences, example));
        }
        self.seen.clear();

        let mut reasons: Vec<Reason> = folded
            .into_iter()
            .map(|((severity, code), (count, example))| Reason::new(severity, code, count, example))
            .collect();
        reasons.sort_by_key(|reason| (reason.severity(), Reverse(reason.count()), reason.code()));
        reasons
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        code::{broker_code::BrokerCode, diagnostic_code::DiagnosticCode, schema_code::SchemaCode},
        context_field::ContextField,
        detail::Detail,
        diagnostic::Diagnostic,
        diagnostic_builder::DiagnosticBuilder,
        reason::Sighting,
        reason_tally::ReasonTally,
        severity::Severity,
    };
    use http::StatusCode;
    use std::num::NonZeroU64;

    fn rejection(status: u16, detail: &str, occurrences: u64) -> Diagnostic {
        DiagnosticBuilder::new(
            Severity::Error,
            DiagnosticCode::Broker(BrokerCode::EntityRejected),
            format!("Broker rejected {occurrences} entities"),
        )
        .with_context(ContextField::HttpStatus(StatusCode::from_u16(status).unwrap()))
        .with_context(ContextField::Detail(Detail::new(detail)))
        .with_occurrences(NonZeroU64::new(occurrences).unwrap())
        .build()
    }

    fn nonconformance(count: u64) -> Diagnostic {
        DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Schema(SchemaCode::Nonconformant), "entities do not conform")
            .with_context(ContextField::Detail(Detail::new("/temperature: required property missing")))
            .with_occurrences(NonZeroU64::new(count).unwrap())
            .build()
    }

    #[test]
    fn the_first_report_is_new_and_every_repeat_is_not() {
        let tally = ReasonTally::new();

        assert_eq!(tally.record(&rejection(422, "bad date", 1)), Sighting::First);
        assert_eq!(tally.record(&rejection(422, "bad date", 1)), Sighting::Repeat);
        assert_eq!(tally.record(&rejection(422, "bad date", 1)), Sighting::Repeat);
    }

    #[test]
    fn the_count_sums_occurrences_rather_than_reports() {
        let tally = ReasonTally::new();
        let _ = tally.record(&rejection(422, "bad date", 100));
        let _ = tally.record(&rejection(422, "bad date", 42));

        let reasons = tally.drain();

        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].count(), 142);
    }

    #[test]
    fn distinct_identities_fold_into_one_row_per_code() {
        let tally = ReasonTally::new();
        let _ = tally.record(&rejection(422, "bad date", 100));
        let _ = tally.record(&rejection(409, "already exists", 42));

        let reasons = tally.drain();

        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].code(), DiagnosticCode::Broker(BrokerCode::EntityRejected));
        assert_eq!(reasons[0].count(), 142);
    }

    #[test]
    fn rows_order_by_severity_then_by_descending_count() {
        let tally = ReasonTally::new();
        let _ = tally.record(&nonconformance(1000));
        let _ = tally.record(&rejection(422, "bad date", 5));

        let reasons = tally.drain();

        assert_eq!(reasons[0].severity(), Severity::Error);
        assert_eq!(reasons[1].severity(), Severity::Warning);
        assert_eq!(reasons[1].count(), 1000);
    }

    #[test]
    fn the_example_is_the_smallest_explanation_whatever_order_it_arrived_in() {
        let forwards = ReasonTally::new();
        let _ = forwards.record(&rejection(422, "a bad date", 1));
        let _ = forwards.record(&rejection(409, "z already exists", 1));

        let backwards = ReasonTally::new();
        let _ = backwards.record(&rejection(409, "z already exists", 1));
        let _ = backwards.record(&rejection(422, "a bad date", 1));

        assert_eq!(forwards.drain()[0].example(), "a bad date");
        assert_eq!(backwards.drain()[0].example(), "a bad date");
    }

    #[test]
    fn draining_lets_a_second_cycle_report_afresh() {
        let tally = ReasonTally::new();
        assert_eq!(tally.record(&rejection(422, "bad date", 1)), Sighting::First);
        let _ = tally.drain();

        assert!(tally.is_empty());
        assert_eq!(tally.record(&rejection(422, "bad date", 1)), Sighting::First);
    }
}
