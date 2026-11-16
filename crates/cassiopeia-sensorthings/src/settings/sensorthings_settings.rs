use crate::{
    settings::{
        collect_limits::CollectLimits,
        features_of_interest::FeaturesOfInterest,
        historical_locations::HistoricalLocations,
        multi_datastreams::MultiDatastreams,
        page_size::PageSize,
        read_mode::ReadMode,
        request_delay::RequestDelay,
        stream_filter::StreamFilter,
    },
    stream_kind::StreamKind,
};
use getset::{CopyGetters, Getters};
use serde::{Deserialize, Deserializer, Serialize, de::Error};
use typed_builder::TypedBuilder;

/// How a remote SensorThings input is read: the `sensorthings` block of a manifest input.
///
/// Every member is optional. Without one, every Datastream and no `MultiDatastream` is read with
/// `read: "latest"`, in pages of 1000, up to the default limits, without a wait between requests,
/// without features of interest and without `HistoricalLocations`. A command that reads a server builds the settings it reads with,
/// each member it does not state at that default.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Getters, CopyGetters, TypedBuilder)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SensorThingsSettings {
    /// The Datastreams read, selected by an `OData` `$filter`; every Datastream when `None`.
    #[serde(default, deserialize_with = "datastream_filter", skip_serializing_if = "Option::is_none")]
    #[getset(get = "pub")]
    #[builder(default)]
    datastreams: Option<StreamFilter>,

    /// The `MultiDatastreams` read; none unless the block selects some.
    #[serde(default)]
    #[getset(get = "pub")]
    #[builder(default)]
    multi_datastreams: MultiDatastreams,

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

    /// Whether every Observation is requested with its `FeatureOfInterest`.
    #[serde(default)]
    #[getset(get_copy = "pub")]
    #[builder(default)]
    features_of_interest: FeaturesOfInterest,

    /// Whether a range or incremental read requests the `HistoricalLocations` of every Thing it lists.
    #[serde(default)]
    #[getset(get_copy = "pub")]
    #[builder(default)]
    historical_locations: HistoricalLocations,
}

impl SensorThingsSettings {
    /// The kinds of data stream a read reads, Datastreams first: Datastreams always, and
    /// `MultiDatastreams` when the settings select some.
    pub fn kinds(&self) -> impl Iterator<Item = StreamKind> {
        let multi = match self.multi_datastreams {
            MultiDatastreams::None => None,
            MultiDatastreams::Every | MultiDatastreams::Filtered(_) => Some(StreamKind::MultiDatastream),
        };
        [Some(StreamKind::Datastream), multi].into_iter().flatten()
    }

    /// The filter that selects the data streams of `kind` a read reads, or `None` when it reads every
    /// one of them it reads at all.
    #[must_use]
    pub const fn filter(&self, kind: StreamKind) -> Option<&StreamFilter> {
        match (kind, &self.multi_datastreams) {
            (StreamKind::Datastream, _) => self.datastreams.as_ref(),
            (StreamKind::MultiDatastream, MultiDatastreams::Filtered(filter)) => Some(filter),
            (StreamKind::MultiDatastream, MultiDatastreams::None | MultiDatastreams::Every) => None,
        }
    }
}

/// Reads the `datastreams` member: a filter over Datastreams, or `null` for none, so a refusal names
/// the member it refuses.
fn datastream_filter<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<StreamFilter>, D::Error> {
    Option::<String>::deserialize(deserializer)?
        .map(|text| StreamFilter::new(StreamKind::Datastream, text))
        .transpose()
        .map_err(D::Error::custom)
}

#[cfg(test)]
mod tests {
    use crate::{
        read_kind::ReadKind,
        settings::{
            features_of_interest::FeaturesOfInterest,
            historical_locations::HistoricalLocations,
            multi_datastreams::MultiDatastreams,
            read_mode::ReadMode,
            request_delay::RequestDelay,
            sensorthings_settings::SensorThingsSettings,
            stream_filter::StreamFilter,
        },
        stream_kind::StreamKind,
    };
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn settings(json: &str) -> Result<SensorThingsSettings, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn settings_default_to_a_latest_read_of_every_datastream_and_no_multi_datastream() {
        let settings = settings("{}").unwrap();

        assert_eq!(settings.read(), &ReadMode::Latest);
        assert_eq!(settings.page_size().get(), 1000);
        assert_eq!(settings.limits().pages().get(), 1000);
        assert_eq!(settings.limits().observations().get(), 500_000);
        assert_eq!(settings.delay(), None);
        assert_eq!(settings.datastreams(), &None);
        assert_eq!(settings.multi_datastreams(), &MultiDatastreams::None);
        assert_eq!(settings.features_of_interest(), FeaturesOfInterest::Omitted);
        assert_eq!(settings.historical_locations(), HistoricalLocations::Omitted);
        assert_eq!(settings.kinds().collect::<Vec<_>>(), [StreamKind::Datastream]);
        assert_eq!(settings, SensorThingsSettings::default());
    }

    #[test]
    fn stated_settings_are_read() {
        let settings = settings(
            r#"{"read": {"range": {"last": "24h"}}, "pageSize": 200, "datastreams": "x eq 1", "multiDatastreams": "y eq 2", "delay": "200ms", "limits": {"pages": 10, "observations": 1000}, "featuresOfInterest": true, "historicalLocations": true}"#,
        )
        .unwrap();

        assert_eq!(settings.read().kind(), ReadKind::Range);
        assert_eq!(settings.page_size().get(), 200);
        assert_eq!(settings.datastreams().as_ref().map(StreamFilter::as_str), Some("x eq 1"));
        assert_eq!(settings.filter(StreamKind::Datastream).map(StreamFilter::as_str), Some("x eq 1"));
        assert_eq!(settings.filter(StreamKind::MultiDatastream).map(StreamFilter::as_str), Some("y eq 2"));
        assert_eq!(settings.delay().map(RequestDelay::get), Some(Duration::from_millis(200)));
        assert_eq!(settings.limits().pages().get(), 10);
        assert_eq!(settings.limits().observations().get(), 1000);
        assert_eq!(settings.features_of_interest(), FeaturesOfInterest::Requested);
        assert_eq!(settings.historical_locations(), HistoricalLocations::Requested);
    }

    #[test]
    fn multi_datastreams_are_read_after_datastreams_once_selected() {
        for json in [r#"{"multiDatastreams": true}"#, r#"{"multiDatastreams": "startswith(name,'ws-')"}"#] {
            assert_eq!(
                settings(json).unwrap().kinds().collect::<Vec<_>>(),
                [StreamKind::Datastream, StreamKind::MultiDatastream],
                "{json}"
            );
        }
        assert_eq!(settings(r#"{"multiDatastreams": true}"#).unwrap().filter(StreamKind::MultiDatastream), None);
        assert_eq!(
            settings(r#"{"multiDatastreams": false}"#).unwrap().kinds().collect::<Vec<_>>(),
            [StreamKind::Datastream]
        );
    }

    #[test]
    fn settings_out_of_range_or_unknown_are_refused() {
        for json in [
            r#"{"paging": {}}"#,
            r#"{"pageSize": 0}"#,
            r#"{"pageSize": 10001}"#,
            r#"{"delay": "61s"}"#,
            r#"{"limits": {"pages": 0}}"#,
            r#"{"featuresOfInterest": "yes"}"#,
            r#"{"featureOfInterest": true}"#,
            r#"{"historicalLocations": "yes"}"#,
            r#"{"historicalLocation": true}"#,
            r#"{"multiDatastreams": 1}"#,
            r#"{"multiDatastream": true}"#,
        ] {
            assert!(settings(json).is_err(), "{json}");
        }
    }

    #[test]
    fn a_refused_filter_names_its_setting() {
        let datastreams = settings(r#"{"datastreams": " "}"#).unwrap_err();
        let multi = settings(r#"{"multiDatastreams": " "}"#).unwrap_err();

        assert!(datastreams.to_string().contains("a `datastreams` filter"), "{datastreams}");
        assert!(multi.to_string().contains("a `multiDatastreams` filter"), "{multi}");
    }

    #[test]
    fn built_settings_default_every_member_they_do_not_state() {
        let filter = StreamFilter::new(StreamKind::Datastream, "endswith(name,'-pm10')".to_owned()).unwrap();

        let built = SensorThingsSettings::builder().datastreams(Some(filter)).build();

        assert_eq!(built, settings(r#"{"datastreams": "endswith(name,'-pm10')"}"#).unwrap());
        assert_eq!(SensorThingsSettings::builder().build(), SensorThingsSettings::default());
    }

    #[test]
    fn stated_settings_round_trip_through_serialization() {
        let stated =
            settings(r#"{"read": {"range": {"last": "1h"}}, "datastreams": "endswith(name,'-pm10')", "multiDatastreams": true, "pageSize": 10, "delay": "200ms", "featuresOfInterest": true, "historicalLocations": true}"#)
                .unwrap();

        let json = serde_json::to_string(&stated).unwrap();

        assert_eq!(settings(&json).unwrap(), stated);
    }
}
