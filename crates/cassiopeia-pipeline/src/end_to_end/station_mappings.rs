//! Station mapping documents shared by several end-to-end scenarios.

pub(crate) const STATION_TEMPERATURE: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ id }}" },
    attributes: { temperature: { source: "{{ temperature }}" } },
}"#;

pub(crate) const STATION_NAME: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ id }}" },
    attributes: { name: { source: "{{ name }}" } },
}"#;

pub(crate) const STATION_READINGS: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ id }}" },
    attributes: {
        temperature: { source: "{% if temperature is defined %}{{ temperature }}{% endif %}" },
        windSpeed: { source: "{% if wind is defined %}{{ wind }}{% endif %}" },
        location: { source: "{{ geometry }}", type: "GeoProperty" },
    },
}"#;

pub(crate) const STATION_READINGS_AND_LEVEL: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ id }}" },
    attributes: {
        temperature: { source: "{% if temperature is defined %}{{ temperature }}{% endif %}" },
        windSpeed: { source: "{% if wind is defined %}{{ wind }}{% endif %}" },
        height: { source: "{{ level }}" },
    },
}"#;
