use crate::end_to_end::{
    harness::{RunSchemas, STORES, attempt_declared_end_to_end, entity_ids},
    sensorthings::{entity, fixture},
};
use cassiopeia_diagnostic::code::{collect_code::CollectCode, diagnostic_code::DiagnosticCode};
use mockito::{Mock, ServerGuard};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The station with a `pm10` data stream, as the inventory writes it from a read of the mock server.
const STATION: &str = "urn:ngsi-ld:Device:sta-127001-1";

/// The station without data streams, as the inventory writes it from a read of the mock server.
const UNCONNECTED_STATION: &str = "urn:ngsi-ld:Device:sta-127001-2";

/// The schemas the inventory validates against: the run checks what the mapping writes, not what a
/// `Device` schema requires of it.
const INVENTORY_SCHEMAS: RunSchemas<'static> = RunSchemas::Permissive(&["dataModel.Device/Device"]);

/// A Thing as a server lists it, with one `GeoJSON` Location at `coordinates`.
fn thing(id: u64, name: &str, coordinates: [f64; 2]) -> Value {
    json!({
        "@iot.id": id,
        "name": name,
        "description": format!("Air quality station {name}"),
        "Locations": [{"@iot.id": id * 100 + 1, "encodingType": "application/geo+json", "location": {"type": "Point", "coordinates": coordinates}}],
    })
}

/// Serves `body` at `path_and_query`, once.
fn serve(server: &mut ServerGuard, path_and_query: &str, body: &Value) -> Mock {
    server.mock("GET", path_and_query).with_body(body.to_string()).expect(1).create()
}

/// The manifest input that reads the mock server `server` with the `sensorthings` block `settings`
/// through the mapping that keeps an inventory of its stations, each with its name and location.
fn declared(server: &ServerGuard, settings: &str) -> String {
    format!(
        r#"{{ source: "{}/v1.1", format: "sensorthings", mapping: "{}", sensorthings: {settings} }}"#,
        server.url(),
        fixture("station-inventory.json5").display()
    )
}

#[test]
fn a_station_without_data_streams_is_written_beside_the_stations_with_readings() {
    for store in STORES {
        let mut server = mockito::Server::new();
        let stationsplein = thing(1, "Stationsplein", [5.4797, 51.4433]);
        let pm10 = json!({
            "@iot.id": 11,
            "name": "station-1 pm10",
            "unitOfMeasurement": {"name": "microgram per cubic metre", "symbol": "µg/m³", "definition": "http://unitsofmeasure.org/ucum.html#para-30"},
            "Thing": stationsplein,
            "ObservedProperty": {"@iot.id": 1, "name": "pm10"},
            "Observations": [{"@iot.id": 501, "phenomenonTime": "2026-11-16T12:00:00Z", "result": 18.2}],
        });
        let mocks = [
            serve(
                &mut server,
                "/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc&$top=1000",
                &json!({ "value": [pm10] }),
            ),
            serve(
                &mut server,
                "/v1.1/Things?$expand=Locations&$orderby=id%20asc&$top=1000",
                &json!({"value": [stationsplein, thing(2, "Kennedylaan", [5.4812, 51.4402])]}),
            ),
        ];

        let (result, outcome) = attempt_declared_end_to_end(store, &declared(&server, "{ things: true }"), &INVENTORY_SCHEMAS, "");

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        assert_eq!(entity_ids(&outcome), [STATION, UNCONNECTED_STATION], "{store:?}");
        for (id, name, coordinates) in [
            (STATION, "Stationsplein", [5.4797, 51.4433]),
            (UNCONNECTED_STATION, "Kennedylaan", [5.4812, 51.4402]),
        ] {
            let station = entity(&outcome, id);
            assert_eq!(station["name"]["value"], name, "{store:?} {id}");
            assert_eq!(station["description"]["value"], format!("Air quality station {name}"), "{store:?} {id}");
            assert_eq!(
                station["location"]["value"],
                json!({"type": "Point", "coordinates": coordinates}),
                "{store:?} {id}"
            );
        }
        assert!(outcome.diagnostics.is_empty(), "{store:?}: {:?}", outcome.diagnostics);
    }
}

#[test]
fn a_page_limit_keeps_the_things_listed_and_names_the_next_link() {
    let mut server = mockito::Server::new();
    let next = format!("{}/v1.1/Things?$skip=2", server.url());
    let mocks = [
        serve(
            &mut server,
            "/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc&$top=2",
            &json!({"value": []}),
        ),
        serve(
            &mut server,
            "/v1.1/Things?$expand=Locations&$orderby=id%20asc&$top=2",
            &json!({"value": [thing(1, "Stationsplein", [5.4797, 51.4433]), thing(2, "Kennedylaan", [5.4812, 51.4402])], "@iot.nextLink": next}),
        ),
    ];
    let untouched = server.mock("GET", "/v1.1/Things?$skip=2").expect(0).create();

    let (result, outcome) = attempt_declared_end_to_end(
        STORES[0],
        &declared(&server, "{ things: true, pageSize: 2, limits: { pages: 1 } }"),
        &INVENTORY_SCHEMAS,
        "",
    );

    result.unwrap();
    mocks.iter().for_each(Mock::assert);
    untouched.assert();
    assert_eq!(entity_ids(&outcome), [STATION, UNCONNECTED_STATION]);
    let reported = outcome.coded(DiagnosticCode::Collect(CollectCode::LimitReached));
    assert_eq!(reported.len(), 1, "{:?}", outcome.diagnostics);
    assert_eq!(
        reported[0].headline,
        format!(
            "In `{}/v1.1`, `limits.pages: 1` stopped the read before it collected all it was asked for, so this cycle carries on with what it collected; the next link `{next}` was not followed",
            server.url()
        )
    );
}
