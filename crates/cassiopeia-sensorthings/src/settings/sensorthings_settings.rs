use crate::settings::{
    collect_limits::CollectLimits,
    datastream_filter::DatastreamFilter,
    page_size::PageSize,
    read_mode::ReadMode,
    request_delay::RequestDelay,
};
use getset::{CopyGetters, Getters};
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// How a remote SensorThings input is read: the `sensorthings` block of a manifest input.
///
/// Every member is optional. Without one, every data stream is read with `read: "latest"`, in pages
/// of 1000, up to the default limits and without a wait between requests. A command that reads a
/// server builds the settings it reads with, each member it does not state at that default.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Getters, CopyGetters, TypedBuilder)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SensorThingsSettings {
    /// The data streams read, selected by an `OData` `$filter`; every data stream when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[getset(get = "pub")]
    #[builder(default)]
    datastreams: Option<DatastreamFilter>,

    /// What the read collects.
    #[serde(default)]
    #[getset(get = "pub")]
    #[builder(default)]
    read: ReadMode,

    /// The `$top` of every request.
    #[serde(default)]
    #[getset(get_copy = "pub")]
    #[builder(default)]
    page_size: PageSize,

    /// How far one cycle goes.
    #[serde(default)]
    #[getset(get_copy = "pub")]
    #[builder(default)]
    limits: CollectLimits,

    /// The wait between consecutive requests; none when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[getset(get_copy = "pub")]
    #[builder(default)]
    delay: Option<RequestDelay>,
}

#[cfg(test)]
mod tests {
    use crate::settings::{datastream_filter::DatastreamFilter, read_mode::ReadMode, request_delay::RequestDelay, sensorthings_settings::SensorThingsSettings};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn settings(json: &str) -> Result<SensorThingsSettings, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn settings_default_to_a_latest_read_of_every_data_stream() {
        let settings = settings("{}").unwrap();

        assert_eq!(settings.read(), &ReadMode::Latest);
        assert_eq!(settings.page_size().get(), 1000);
        assert_eq!(settings.limits().pages().get(), 1000);
        assert_eq!(settings.limits().observations().get(), 500_000);
        assert_eq!(settings.delay(), None);
        assert_eq!(settings.datastreams(), &None);
        assert_eq!(settings, SensorThingsSettings::default());
    }

    #[test]
    fn stated_settings_are_read() {
        let settings =
            settings(r#"{"read": "latest", "pageSize": 200, "datastreams": "x eq 1", "delay": "200ms", "limits": {"pages": 10, "observations": 1000}}"#)
                .unwrap();

        assert_eq!(settings.read(), &ReadMode::Latest);
        assert_eq!(settings.page_size().get(), 200);
        assert_eq!(settings.datastreams().as_ref().map(DatastreamFilter::as_str), Some("x eq 1"));
        assert_eq!(settings.delay().map(RequestDelay::get), Some(Duration::from_millis(200)));
        assert_eq!(settings.limits().pages().get(), 10);
        assert_eq!(settings.limits().observations().get(), 1000);
    }

    #[test]
    fn settings_out_of_range_or_unknown_are_refused() {
        for json in [
            r#"{"paging": {}}"#,
            r#"{"pageSize": 0}"#,
            r#"{"pageSize": 10001}"#,
            r#"{"delay": "61s"}"#,
            r#"{"limits": {"pages": 0}}"#,
        ] {
            assert!(settings(json).is_err(), "{json}");
        }
    }

    #[test]
    fn built_settings_default_every_member_they_do_not_state() {
        let filter = DatastreamFilter::try_from("endswith(name,'-pm10')".to_owned()).unwrap();

        let built = SensorThingsSettings::builder().datastreams(Some(filter)).build();

        assert_eq!(built, settings(r#"{"datastreams": "endswith(name,'-pm10')"}"#).unwrap());
        assert_eq!(SensorThingsSettings::builder().build(), SensorThingsSettings::default());
    }

    #[test]
    fn stated_settings_round_trip_through_serialization() {
        let stated = settings(r#"{"datastreams": "endswith(name,'-pm10')", "pageSize": 10, "delay": "200ms"}"#).unwrap();

        let json = serde_json::to_string(&stated).unwrap();

        assert_eq!(settings(&json).unwrap(), stated);
    }
}
