use crate::{build_info::default_user_agent, cli::sensorthings::ReadingsArgs, error::Result, shutdown};
use cassiopeia_cli::sensorthings_readings::{ReadingsListing, list_readings};
use cassiopeia_collector::downloader::Downloader;
use cassiopeia_sensorthings::{
    service::SensorThingsService,
    settings::{multi_datastreams::MultiDatastreams, sensorthings_settings::SensorThingsSettings},
};

/// Runs `sensorthings readings`: lists the readings the server `args` names offers, reading only the
/// Datastreams and the `MultiDatastreams` `args` selects, every `MultiDatastream` when it selects
/// none, with the run's `User-Agent` and the `--input-header` values on every request, and every
/// other setting at a run's default.
pub fn list_server_readings(args: &ReadingsArgs) -> Result<ReadingsListing> {
    // The service owns its root and its filters, while the parsed command line keeps its own.
    let multi_datastreams = args.multi_datastreams.clone().map_or(MultiDatastreams::Every, MultiDatastreams::Filtered);
    let settings = SensorThingsSettings::builder()
        .datastreams(args.datastreams.clone())
        .multi_datastreams(multi_datastreams)
        .build();
    let service = SensorThingsService::new(args.root.clone(), settings);
    let downloader = Downloader::new(default_user_agent(), args.headers.request_headers(), shutdown::flag());
    Ok(list_readings(&service, downloader, shutdown::flag())?)
}

#[cfg(test)]
mod tests {
    use crate::{
        cli::{
            Cli,
            Commands,
            sensorthings::{ReadingsArgs, SensorThingsSubcommand},
        },
        readings_setup::list_server_readings,
    };
    use cassiopeia_cli::readings_table::render_readings;
    use clap::Parser;
    use mockito::{Matcher, Mock, ServerGuard};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    /// The path and query of the metadata discovery request of a `sensorthings readings` without
    /// `--datastreams`.
    const METADATA: &str = "/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc&$top=1000";

    /// The arguments of `cassiopeia sensorthings readings` against the mock server's root, followed
    /// by `options`.
    fn readings_args(server: &ServerGuard, options: &[&str]) -> ReadingsArgs {
        let root = format!("{}/v1.1", server.url());
        let command_line = ["cassiopeia", "sensorthings", "readings", root.as_str()]
            .into_iter()
            .chain(options.iter().copied());
        let Commands::SensorThings(command) = Cli::try_parse_from(command_line).unwrap().command else {
            panic!("expected the sensorthings command");
        };
        let SensorThingsSubcommand::Readings(args) = command.command else {
            panic!("expected the readings command");
        };
        args
    }

    /// A served Datastream `id` of the Thing `thing`, whose readings are named `name`, with a unit
    /// written as `symbol`.
    fn datastream(id: u64, thing: u64, name: &str, symbol: &str) -> Value {
        json!({
            "@iot.id": id,
            "name": format!("stream {id}"),
            "unitOfMeasurement": {"name": symbol, "symbol": symbol, "definition": ""},
            "Thing": {"@iot.id": thing, "name": format!("thing {thing}"), "Locations": []},
            "ObservedProperty": {"@iot.id": 1, "name": name, "definition": ""},
        })
    }

    /// A last page holding `datastreams`.
    fn page(datastreams: &[Value]) -> String {
        json!({"value": datastreams}).to_string()
    }

    /// Serves, once, a service root document that lists the collections `names`.
    fn serve_root(server: &mut ServerGuard, names: &[&str]) -> Mock {
        let collections: Vec<Value> = names
            .iter()
            .map(|name| json!({"name": name, "url": format!("https://h.example/v1.1/{name}")}))
            .collect();
        server.mock("GET", "/v1.1").with_body(page(&collections)).expect(1).create()
    }

    /// The mock of every request for `MultiDatastreams`, which a server whose root lists none is never
    /// sent.
    fn untouched_multi_datastreams(server: &mut ServerGuard) -> Mock {
        server.mock("GET", Matcher::Regex(r"^/v1\.1/MultiDatastreams".to_owned())).expect(0).create()
    }

    #[test]
    fn readings_lists_one_row_per_observed_property() {
        let mut server = mockito::Server::new();
        let discovery = server
            .mock("GET", METADATA)
            .with_body(page(&[
                datastream(1, 1, "pm10", "ug/m3"),
                datastream(2, 2, "pm10", "ug/m3"),
                datastream(3, 1, "no2", "xyzzy"),
            ]))
            .expect(1)
            .create();
        let root = serve_root(&mut server, &["Datastreams"]);
        let untouched = untouched_multi_datastreams(&mut server);

        let listing = list_server_readings(&readings_args(&server, &[])).unwrap();

        discovery.assert();
        root.assert();
        untouched.assert();
        assert_eq!(
            render_readings(listing.readings()),
            "reading  unit   unit code  data streams  multi data streams  things with repeats\n\
             no2      xyzzy  unknown    1             0                   0\n\
             pm10     ug/m3  GQ         2             0                   0"
        );
        assert_eq!(listing.limits_reached(), &[]);
    }

    #[test]
    fn readings_sends_the_input_headers() {
        let mut server = mockito::Server::new();
        let second = format!("{}/v1.1/Datastreams?$skip=1", server.url());
        let pages = [
            server
                .mock("GET", METADATA)
                .match_header("authorization", "Bearer t")
                .with_body(json!({"value": [datastream(1, 1, "pm10", "ug/m3")], "@iot.nextLink": second}).to_string())
                .expect(1)
                .create(),
            server
                .mock("GET", "/v1.1/Datastreams?$skip=1")
                .match_header("authorization", "Bearer t")
                .with_body(page(&[datastream(2, 1, "pm10", "ug/m3")]))
                .expect(1)
                .create(),
        ];
        serve_root(&mut server, &["Datastreams"]);

        let listing = list_server_readings(&readings_args(&server, &["--input-header", "Authorization: Bearer t"])).unwrap();

        pages.iter().for_each(mockito::Mock::assert);
        assert_eq!(listing.readings()[0].things_with_repeats(), 1);
    }

    #[test]
    fn the_datastreams_option_selects_the_data_streams_read() {
        let mut server = mockito::Server::new();
        let discovery = server
            .mock(
                "GET",
                "/v1.1/Datastreams?$filter=startswith(name,%27ILM_I68-%27)&$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc&$top=1000",
            )
            .with_body(page(&[]))
            .expect(1)
            .create();
        serve_root(&mut server, &["Datastreams"]);

        let listing = list_server_readings(&readings_args(&server, &["--datastreams", "startswith(name,'ILM_I68-')"])).unwrap();

        discovery.assert();
        assert_eq!(listing.readings().as_slice(), []);
    }

    #[test]
    fn the_multi_datastreams_option_selects_the_multi_datastreams_read_when_the_root_lists_them() {
        let mut server = mockito::Server::new();
        let wind = json!({
            "@iot.id": 4,
            "name": "ws-4",
            "Thing": {"@iot.id": 1, "Locations": []},
            "ObservedProperties": [{"@iot.id": 11, "name": "windSpeed"}, {"@iot.id": 12, "name": "windDirection"}],
            "unitOfMeasurements": [{"symbol": "m/s"}, {"symbol": "deg"}],
            "multiObservationDataTypes": ["OM_Measurement", "OM_Measurement"],
        });
        let mocks = [
            server
                .mock("GET", METADATA)
                .with_body(page(&[datastream(1, 1, "pm10", "ug/m3")]))
                .expect(1)
                .create(),
            serve_root(&mut server, &["Datastreams", "MultiDatastreams"]),
            server
                .mock(
                    "GET",
                    "/v1.1/MultiDatastreams?$filter=startswith(name,%27ws-%27)&$expand=Thing/Locations,ObservedProperties,Sensor&$orderby=id%20asc&$top=1000",
                )
                .with_body(page(&[wind]))
                .expect(1)
                .create(),
        ];

        let listing = list_server_readings(&readings_args(&server, &["--multi-datastreams", "startswith(name,'ws-')"])).unwrap();

        mocks.iter().for_each(Mock::assert);
        assert_eq!(
            render_readings(listing.readings()).lines().skip(1).collect::<Vec<_>>(),
            [
                "pm10           ug/m3  GQ         1             0                   0",
                "windDirection  deg    DD         0             1                   0",
                "windSpeed      m/s    MTS        0             1                   0",
            ]
        );
    }

    #[test]
    fn a_blank_multi_datastreams_option_is_refused_naming_it() {
        let error = Cli::try_parse_from(["cassiopeia", "sensorthings", "readings", "https://h.example/v1.1", "--multi-datastreams", " "]).unwrap_err();

        assert!(error.to_string().contains("a `multiDatastreams` filter is an OData expression"), "{error}");
    }
}
