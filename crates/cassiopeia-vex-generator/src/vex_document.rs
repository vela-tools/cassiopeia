//! The OpenVEX v0.2.0 document the generator writes, and reads back for the timestamps and the version it
//! carries over.
//!
//! Members are declared in the order the document lists them, so the written text keeps one layout. The
//! member names, and the status and justification tokens, are the ones the OpenVEX specification spells.

use crate::vulnerability_id::VulnerabilityId;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use url::Url;

/// An OpenVEX document.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VexDocument {
    /// The OpenVEX context the document follows.
    #[serde(rename = "@context")]
    pub context: Url,
    /// Where the document is published.
    #[serde(rename = "@id")]
    pub id: Url,
    /// Who issues the rulings.
    pub author: String,
    /// When the document was first issued.
    pub timestamp: Timestamp,
    /// When a statement last changed.
    pub last_updated: Timestamp,
    /// The document's version, raised whenever a statement changes.
    pub version: u64,
    /// The statements, one per identifier of each ruled advisory.
    pub statements: Vec<Statement>,
}

/// One ruling on one vulnerability in the named products.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    /// The vulnerability ruled on.
    pub vulnerability: Vulnerability,
    /// The products the ruling covers.
    pub products: Vec<Product>,
    /// How the vulnerability affects the products.
    pub status: Status,
    /// Why the products are not affected.
    pub justification: Justification,
    /// The ruling in prose, the reason `deny.toml` gives for ignoring the advisory.
    #[serde(rename = "impact_statement")]
    pub impact: String,
}

/// The vulnerability a statement rules on.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vulnerability {
    /// The identifier the statement is named by.
    pub name: VulnerabilityId,
    /// The vulnerability's other identifiers; empty when it has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<VulnerabilityId>,
}

/// A product a statement covers, with the components inside it the ruling is about.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Product {
    /// The product's package URL.
    #[serde(rename = "@id")]
    pub id: String,
    /// The components inside the product the ruling is about.
    pub subcomponents: Vec<Subcomponent>,
}

/// A component inside a product.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subcomponent {
    /// The component's package URL.
    #[serde(rename = "@id")]
    pub id: String,
}

/// How a vulnerability affects a product. An advisory `deny.toml` ignores affects none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// No remediation is needed.
    NotAffected,
}

/// Why a product is not affected, as the OpenVEX specification spells the justification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Justification {
    /// The vulnerable component is not in the product.
    ComponentNotPresent,
    /// The component is in the product, its vulnerable code is not.
    VulnerableCodeNotPresent,
    /// The vulnerable code is in the product and never runs.
    VulnerableCodeNotInExecutePath,
    /// The vulnerable code runs, on input no adversary controls.
    VulnerableCodeCannotBeControlledByAdversary,
    /// The product mitigates the vulnerability itself.
    InlineMitigationsAlreadyExist,
}

#[cfg(test)]
mod tests {
    use crate::vex_document::{Justification, Status};

    #[test]
    fn the_status_and_justification_tokens_are_spelt_as_openvex_spells_them() {
        assert_eq!(serde_json::to_string(&Status::NotAffected).unwrap(), "\"not_affected\"");
        assert_eq!(
            serde_json::to_string(&Justification::VulnerableCodeNotPresent).unwrap(),
            "\"vulnerable_code_not_present\""
        );
        assert_eq!(
            serde_json::from_str::<Justification>("\"component_not_present\"").unwrap(),
            Justification::ComponentNotPresent
        );
    }
}
