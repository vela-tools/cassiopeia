use crate::end_to_end::harness::{RunBinding, RunOutcome, STORES, run_csv_end_to_end};
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode},
    context_field::ContextField,
};
use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// Two numerical models' forecasts for one station, the second tagged with the `datasetId` its row
/// names.
const FORECAST_MAPPING: &str = r#"{
    version: "v4",
    dataModel: "WeatherForecast",
    identity: { entityName: "{{ station }}" },
    attributes: {
        temperature: {
            transformation: "float",
            instances: [
                { source: "{{ ecmwf }}", properties: { datasetId: { source: "urn:ngsi-ld:Dataset:ecmwf" } } },
                { source: "{{ gfs }}", properties: { datasetId: { source: "{{ model }}" } } },
            ],
        },
    },
}"#;

/// One station whose second model names its dataset in text that is no URI.
const FORECASTS: &str = "station,ecmwf,gfs,model\nS1,21.5,22.0,gfs model\n";

/// A single `forecast` whose `datasetId` each row names, beside a `humidity` whose literal
/// `datasetId` is `@none`.
const STATION_MAPPING: &str = r#"{
    version: "v4",
    dataModel: "WeatherForecast",
    identity: { entityName: "{{ code }}" },
    attributes: {
        forecast: { source: "{{ forecast }}", transformation: "float", properties: { datasetId: { source: "{{ dataset }}" } } },
        humidity: { source: "{{ humidity }}", transformation: "float", properties: { datasetId: { source: "@none" } } },
    },
}"#;

/// Five stations: an absolute URI that is no URN, a relative reference, `@none`, an empty field,
/// and a URN.
const STATIONS: &str = "code,forecast,dataset,humidity\n\
    S1,21.5,https://example.org/datasets/gfs,40\n\
    S2,22.5,datasets/gfs,41\n\
    S3,23.5,@none,42\n\
    S4,24.5,,43\n\
    S5,25.5,urn:ngsi-ld:Dataset:gfs,44\n";

/// The written entity whose id ends in `code`.
fn entity<'a>(outcome: &'a RunOutcome, code: &str) -> &'a Value {
    let id = format!("urn:ngsi-ld:WeatherForecast:{code}");
    outcome
        .entities
        .iter()
        .find(|entity| entity["id"] == id.as_str())
        .unwrap_or_else(|| panic!("{id} written: {:?}", outcome.entities))
}

/// The one `extractor-dataset-id-not-a-uri` warning the run reported, checked to name `attribute`,
/// to quote `text`, and to stand for `occurrences` dropped declarations.
fn assert_one_warning(outcome: &RunOutcome, attribute: &str, text: &str, occurrences: u64) {
    let refused = outcome.coded(DiagnosticCode::Extractor(ExtractorCode::DatasetIdNotAUri));
    assert_eq!(refused.len(), 1, "{:?}", outcome.diagnostics);
    assert_eq!(refused[0].occurrences, occurrences);
    assert_eq!(
        refused[0].defining,
        [ContextField::Attribute(AttributePath::top_level(NameBuf::new(attribute).unwrap()))]
    );
    assert!(refused[0].headline.contains(&format!("`{text}`")), "{}", refused[0].headline);
}

#[test]
fn an_instance_whose_dataset_id_is_no_uri_is_dropped_with_one_warning() {
    for store in STORES {
        let outcome = run_csv_end_to_end(store, FORECASTS, &RunBinding::Mapping(FORECAST_MAPPING), &["WeatherForecast"], "");

        assert_eq!(
            entity(&outcome, "S1")["temperature"],
            json!([{ "type": "Property", "value": 21.5, "datasetId": "urn:ngsi-ld:Dataset:ecmwf" }]),
            "{store:?}"
        );
        assert_one_warning(&outcome, "temperature", "gfs model", 1);
        assert_eq!(outcome.warnings, 1, "{store:?}: {:?}", outcome.diagnostics);
    }
}

#[test]
fn a_single_attribute_keeps_a_uri_dataset_id_drops_one_that_is_no_uri_and_reads_none_as_the_default() {
    for store in STORES {
        let outcome = run_csv_end_to_end(store, STATIONS, &RunBinding::Mapping(STATION_MAPPING), &["WeatherForecast"], "");

        assert_eq!(entity(&outcome, "S1")["forecast"]["datasetId"], "https://example.org/datasets/gfs", "{store:?}");
        assert!(entity(&outcome, "S2").get("forecast").is_none(), "{store:?}: {:?}", entity(&outcome, "S2"));
        for (code, value) in [("S3", 23.5), ("S4", 24.5)] {
            let forecast = &entity(&outcome, code)["forecast"];
            assert_eq!(forecast["value"], json!(value), "{store:?} {code}");
            assert!(forecast.get("datasetId").is_none(), "{store:?} {code}: {forecast:?}");
        }
        assert_eq!(entity(&outcome, "S5")["forecast"]["datasetId"], "urn:ngsi-ld:Dataset:gfs", "{store:?}");
        for code in ["S1", "S2", "S3", "S4", "S5"] {
            let humidity = &entity(&outcome, code)["humidity"];
            assert!(humidity.get("value").is_some(), "{store:?} {code}: {humidity:?}");
            assert!(humidity.get("datasetId").is_none(), "{store:?} {code}: {humidity:?}");
        }
        assert_one_warning(&outcome, "forecast", "datasets/gfs", 1);
        assert_eq!(outcome.warnings, 1, "{store:?}: {:?}", outcome.diagnostics);
    }
}
