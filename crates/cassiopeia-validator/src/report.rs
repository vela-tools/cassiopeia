use cassiopeia_ngsi_ld::entity::name::NameBuf;
use serde::Serialize;
use serde_json::Value;
use urn_rs::Urn;

/// Validation failures for a single entity, in jsonschema's list output format.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReportEntry {
    /// The failing entity's URN.
    pub entity_id: Urn,
    /// The failing entity's type.
    pub entity_type: NameBuf,
    /// The jsonschema list-format evaluation output for this entity.
    pub evaluation: Value,
}

/// A full validation report, suitable for writing to disk.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    /// How many entities were checked.
    pub total_entities: u64,
    /// How many of them failed.
    pub failed_entities: u64,
    /// One entry per failing entity.
    pub failures: Vec<ValidationReportEntry>,
}

#[cfg(test)]
mod tests {
    use crate::report::{ValidationReport, ValidationReportEntry};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use serde_json::{Value, json};
    use urn_rs::Urn;

    fn entry() -> ValidationReportEntry {
        ValidationReportEntry {
            entity_id: "urn:ngsi-ld:Sensor:1".parse::<Urn>().unwrap(),
            entity_type: NameBuf::new("Sensor").unwrap(),
            evaluation: json!({ "valid": false }),
        }
    }

    #[test]
    fn an_entry_serializes_its_id_and_type_as_strings_under_camel_case_keys() {
        let value = serde_json::to_value(entry()).unwrap();

        assert_eq!(value["entityId"], Value::String("urn:ngsi-ld:Sensor:1".to_string()));
        assert_eq!(value["entityType"], Value::String("Sensor".to_string()));
    }

    #[test]
    fn a_report_counts_its_entities_and_carries_its_failures() {
        let report = ValidationReport {
            total_entities: 10,
            failed_entities: 1,
            failures: vec![entry()],
        };
        let value = serde_json::to_value(report).unwrap();

        assert_eq!(value["totalEntities"], 10);
        assert_eq!(value["failedEntities"], 1);
        assert_eq!(value["failures"].as_array().unwrap().len(), 1);
    }
}
