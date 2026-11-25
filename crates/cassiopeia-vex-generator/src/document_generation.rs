//! The VEX document generated from the rulings and the reasons `deny.toml` gives for ignoring their
//! advisories.
//!
//! An advisory gets one statement under each of its identifiers, because some scanners apply only a
//! statement named by the identifier they report, and each statement lists the others as aliases. Every
//! statement names the image in both package URL forms scanners name a pulled image by, with the ruling's
//! crate versions inside, and repeats the ignore's reason as its impact statement.
//!
//! The document keeps the version and both times of the checked-in document while its statements read the
//! same, so generating it again changes nothing. Once a statement changes, the version is raised and the
//! last update is dated at generation.

use crate::{
    crate_version::CrateVersion,
    generator_error::GeneratorError,
    rulings::Ruling,
    vex_document::{Product, Statement, Status, Subcomponent, VexDocument, Vulnerability},
    vulnerability_id::VulnerabilityId,
};
use jiff::Timestamp;
use std::collections::BTreeMap;
use url::Url;

/// The OpenVEX context the document follows.
const OPENVEX_CONTEXT: &str = "https://openvex.dev/ns/v0.2.0";

/// Who issues the rulings.
const AUTHOR: &str = "Cassiopeia";

/// The package URLs naming every digest of `ghcr.io/vela-tools/cassiopeia`. A scanner names a pulled
/// image in the `oci` form, by its name with the whole repository as `repository_url`, or in the `docker`
/// form, by its owner and name with the registry as `repository_url`. With no version and no other
/// qualifier, each covers every digest of the image.
const IMAGE_PRODUCTS: [&str; 2] = [
    "pkg:oci/cassiopeia?repository_url=ghcr.io/vela-tools/cassiopeia",
    "pkg:docker/vela-tools/cassiopeia?repository_url=ghcr.io",
];

/// The VEX document published at `id` that states each of `rulings` with the reason `reasons` records for
/// its advisory. `existing` is the checked-in document, whose version and times carry over while the
/// statements read the same, and `now` the time a change is dated. A ruling whose advisory has no reason
/// is left out.
///
/// # Errors
///
/// Fails when a crate version cannot be spelt as a package URL, or the OpenVEX context is no URL.
pub fn vex_document(
    rulings: &BTreeMap<VulnerabilityId, Ruling>,
    reasons: &BTreeMap<VulnerabilityId, Option<String>>,
    id: Url,
    existing: Option<VexDocument>,
    now: Timestamp,
) -> Result<VexDocument, GeneratorError> {
    let mut statements = Vec::new();
    for (advisory, ruling) in rulings {
        let Some(reason) = reasons.get(advisory).and_then(Option::as_deref) else {
            continue;
        };
        statements.extend(advisory_statements(advisory, ruling, reason)?);
    }
    let (timestamp, last_updated, version) = match existing {
        Some(document) if document.id == id && document.statements == statements => (document.timestamp, document.last_updated, document.version),
        Some(document) => (document.timestamp, now, document.version.saturating_add(1)),
        None => (now, now, 1),
    };
    Ok(VexDocument {
        context: Url::parse(OPENVEX_CONTEXT)?,
        id,
        author: AUTHOR.to_owned(),
        timestamp,
        last_updated,
        version,
        statements,
    })
}

/// The statements of the ruling on `advisory`, one under each of its identifiers, with `reason` as their
/// impact statement.
fn advisory_statements(advisory: &VulnerabilityId, ruling: &Ruling, reason: &str) -> Result<Vec<Statement>, GeneratorError> {
    let subcomponents = ruling.crates.iter().map(CrateVersion::package_url).collect::<Result<Vec<_>, _>>()?;
    let identifiers = [advisory].into_iter().chain(&ruling.aliases).collect::<Vec<_>>();
    Ok(identifiers
        .iter()
        .copied()
        .map(|name| Statement {
            // clone: each identifier names its own statement and is listed in the aliases of every other.
            vulnerability: Vulnerability {
                name: name.clone(),
                aliases: identifiers.iter().copied().filter(|other| *other != name).cloned().collect(),
            },
            products: IMAGE_PRODUCTS
                .iter()
                .map(|&product| Product {
                    id: product.to_owned(),
                    // clone: every product of every statement lists the ruling's crate versions.
                    subcomponents: subcomponents.iter().map(|id| Subcomponent { id: id.clone() }).collect(),
                })
                .collect(),
            status: Status::NotAffected,
            justification: ruling.justification,
            impact: reason.to_owned(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use crate::{advisory_ignores::advisory_ignores, document_generation::vex_document, rulings::parse_rulings};
    use jiff::Timestamp;
    use serde_json::{Value, json};
    use url::Url;

    const RULINGS: &str = "[RUSTSEC-2026-0459]\naliases = [\"CVE-2026-119375\", \"GHSA-72w2-rjgg-q729\"]\njustification = \"vulnerable_code_not_present\"\ncrates = [\"libsqlite3-sys@0.38.2\"]\n";

    const DENY: &str = "[advisories]\nignore = [{ id = \"RUSTSEC-2026-0459\", reason = \"The image compiles no bundled SQLite.\" }]\n";

    fn id() -> Url {
        Url::parse("https://github.com/vela-tools/cassiopeia/blob/main/.vex/cassiopeia.openvex.json").unwrap()
    }

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(second).unwrap()
    }

    fn document(deny: &str, existing: Option<Value>, now: i64) -> Value {
        let existing = existing.map(|existing| serde_json::from_value(existing).unwrap());
        let document = vex_document(&parse_rulings(RULINGS).unwrap(), &advisory_ignores(deny).unwrap(), id(), existing, at(now)).unwrap();
        serde_json::to_value(document).unwrap()
    }

    fn statement(name: &str, aliases: &[&str]) -> Value {
        json!({
            "vulnerability": { "name": name, "aliases": aliases },
            "products": [
                {
                    "@id": "pkg:oci/cassiopeia?repository_url=ghcr.io/vela-tools/cassiopeia",
                    "subcomponents": [{ "@id": "pkg:cargo/libsqlite3-sys@0.38.2" }],
                },
                {
                    "@id": "pkg:docker/vela-tools/cassiopeia?repository_url=ghcr.io",
                    "subcomponents": [{ "@id": "pkg:cargo/libsqlite3-sys@0.38.2" }],
                },
            ],
            "status": "not_affected",
            "justification": "vulnerable_code_not_present",
            "impact_statement": "The image compiles no bundled SQLite.",
        })
    }

    #[test]
    fn an_advisory_is_stated_once_under_each_identifier_with_the_others_as_aliases() {
        assert_eq!(
            document(DENY, None, 1_000),
            json!({
                "@context": "https://openvex.dev/ns/v0.2.0",
                "@id": "https://github.com/vela-tools/cassiopeia/blob/main/.vex/cassiopeia.openvex.json",
                "author": "Cassiopeia",
                "timestamp": "1970-01-01T00:16:40Z",
                "last_updated": "1970-01-01T00:16:40Z",
                "version": 1,
                "statements": [
                    statement("RUSTSEC-2026-0459", &["CVE-2026-119375", "GHSA-72w2-rjgg-q729"]),
                    statement("CVE-2026-119375", &["RUSTSEC-2026-0459", "GHSA-72w2-rjgg-q729"]),
                    statement("GHSA-72w2-rjgg-q729", &["RUSTSEC-2026-0459", "CVE-2026-119375"]),
                ],
            })
        );
    }

    #[test]
    fn a_document_generated_again_without_a_change_keeps_its_version_and_times() {
        let first = document(DENY, None, 1_000);
        assert_eq!(document(DENY, Some(first.clone()), 2_000), first);
    }

    #[test]
    fn a_changed_reason_raises_the_version_and_dates_the_update() {
        let first = document(DENY, None, 1_000);
        let changed = document(&DENY.replace("no bundled SQLite.", "no SQLite from libsqlite3-sys."), Some(first), 2_000);
        assert_eq!(changed["version"], 2);
        assert_eq!(changed["timestamp"], "1970-01-01T00:16:40Z");
        assert_eq!(changed["last_updated"], "1970-01-01T00:33:20Z");
    }

    #[test]
    fn a_ruling_whose_advisory_has_no_reason_is_left_out() {
        let document = document("[advisories]\nignore = [\"RUSTSEC-2026-0459\"]\n", None, 1_000);
        assert_eq!(document["statements"], json!([]));
    }
}
