pub mod attribute;
pub mod error;
pub mod geometry_validation;
pub mod identity;
pub mod mapping;
pub mod observed_at;
pub mod scope;
pub mod target;
pub mod template;
pub mod transformation;
pub mod version;

#[cfg(test)]
mod tests {
    use crate::{mapping::Mapping, observed_at::ObservedAt, template::runner::TemplateRunner};
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn the_public_surface_loads_a_mapping_and_reads_its_observed_at() {
        let document = r#"{
            version: "v4",
            dataModel: "dataModel.Environment/AirQualityObserved",
            identity: { entityName: "Station-{{ id }}" },
            attributes: {
                temperature: {
                    source: "{{ temperature }}",
                    properties: { observedAt: { source: "{{ timestamp }}" } },
                },
            },
        }"#;
        let mut runner = TemplateRunner::new();
        let mapping = Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap();

        assert!(mapping.is_temporal());
        assert_eq!(
            mapping.extract_observed_at(&runner.resolver(), &json!({"timestamp": "2026-04-03T22:00:20Z"})),
            Some(ObservedAt::new("2026-04-03T22:00:20Z"))
        );
    }
}
