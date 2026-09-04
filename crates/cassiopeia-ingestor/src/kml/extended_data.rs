use ::kml::types::Element;
use serde_json::{Map, Value};

/// Extracts properties from `ExtendedData` child elements into `properties`.
///
/// Handles both the `SchemaData`/`SimpleData` shape and direct `Data`/`value`
/// elements, the two ways KML attaches typed attributes to a placemark.
pub fn extract_extended_data(children: &[Element], properties: &mut Map<String, Value>) {
    for child in children {
        if child.name != "ExtendedData" {
            continue;
        }

        for extended_child in &child.children {
            if extended_child.name == "SchemaData" {
                for schema_child in &extended_child.children {
                    if schema_child.name == "SimpleData"
                        && let Some(name) = schema_child.attrs.get("name")
                    {
                        let value = schema_child.content.as_deref().unwrap_or("").to_string();
                        properties.insert(name.clone(), Value::String(value));
                    }
                }
            }

            // Direct Data elements (non-Schema ExtendedData).
            if extended_child.name == "Data"
                && let Some(name) = extended_child.attrs.get("name")
            {
                for data_child in &extended_child.children {
                    if data_child.name == "value" {
                        let value = data_child.content.as_deref().unwrap_or("").to_string();
                        properties.insert(name.clone(), Value::String(value));
                    }
                }
            }
        }
    }
}
