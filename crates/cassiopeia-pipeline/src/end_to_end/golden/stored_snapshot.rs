use crate::end_to_end::golden::case::GoldenCase;
use insta::Snapshot;
use serde::Serialize;
use serde_json::Value;

/// Prints where `written` first departs from what the snapshot `name` of `case` stores, as
/// `mismatch` tells it, naming the case and the snapshot; prints nothing when the two are equal or
/// the case has no such snapshot yet.
///
/// insta shows a difference as a few lines of diff, which need not reach the entity or the
/// diagnostic they belong to, so this is printed before insta's assertion reports the difference
/// and keeps what the run wrote for review.
pub(crate) fn print_mismatch<T: Serialize>(case: &GoldenCase, name: &str, written: &T, mismatch: fn(&Value, &Value) -> Option<String>) {
    if let Some(stored) = stored_value(case, name)
        && let Some(mismatch) = mismatch(&stored, &serde_json::to_value(written).unwrap())
    {
        println!("golden case `{}`, snapshot `{name}`: {mismatch}", case.name().display());
    }
}

/// The value the snapshot `name` of `case` stores, or `None` when the case has no such snapshot yet.
fn stored_value(case: &GoldenCase, name: &str) -> Option<Value> {
    let path = case.directory().join(format!("{name}.snap"));
    path.exists().then(|| {
        let snapshot = Snapshot::from_file(&path).unwrap();
        serde_json::from_str(&snapshot.as_text().unwrap().to_string()).unwrap()
    })
}
