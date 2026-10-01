use ::kml::types::Element;
use serde_json::{Map, Value};

/// Extracts a placemark's time primitive into `properties`.
///
/// KML dates a feature with one of two time primitives: `TimeStamp`, a single instant held in its
/// `when` child, or `TimeSpan`, an interval bounded by optional `begin` and `end` children. `when`
/// becomes `timeStamp`, and a span becomes a `timeSpan` object holding whichever bounds are present,
/// so a mapping reads `{{ properties.timeStamp }}` as an `observedAt` or
/// `{{ properties.timeSpan.begin }}` as the start of a window.
///
/// Values pass through verbatim apart from surrounding whitespace: KML admits a full dateTime with
/// or without an offset, a date, a year-month, or a bare year, and the mapping's `datetime`
/// transformation decides how to read them. An empty `when`, or a span with no non-empty bound,
/// contributes nothing.
pub fn extract_time_primitive(children: &[Element], properties: &mut Map<String, Value>) {
    for child in children {
        if child.name == "TimeStamp"
            && let Some(when) = child_text(child, "when")
        {
            properties.insert("timeStamp".to_string(), Value::String(when));
        }

        if child.name == "TimeSpan" {
            let bounds: Map<String, Value> = ["begin", "end"]
                .into_iter()
                .filter_map(|bound| child_text(child, bound).map(|text| (bound.to_string(), Value::String(text))))
                .collect();
            if !bounds.is_empty() {
                properties.insert("timeSpan".to_string(), Value::Object(bounds));
            }
        }
    }
}

/// The trimmed text of `element`'s first child named `name`, or `None` when it is absent or blank.
fn child_text(element: &Element, name: &str) -> Option<String> {
    let text = element.children.iter().find(|child| child.name == name)?.content.as_deref()?.trim();

    if text.is_empty() { None } else { Some(text.to_string()) }
}
