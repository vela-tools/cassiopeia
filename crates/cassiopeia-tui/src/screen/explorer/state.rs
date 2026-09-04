use crate::{
    constraint::node_constraints,
    error::{Result, TuiError},
    model_picker::ModelPicker,
    schema_node_details::SchemaNodeDetails,
    screen::explorer::tree_path::TreePath,
    theme::Theme,
};
use cassiopeia_ngsi_ld::data_model::DataModel;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use serde_json::Value;
use std::collections::HashMap;
use tui_tree_widget::{TreeItem, TreeState};

/// Which of the explorer's two screens is showing.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ExplorerScreen {
    /// The model-selection list.
    ModelSelection,
    /// The schema tree viewer.
    SchemaViewer,
}

/// Everything the read-only schema explorer keeps between frames.
pub struct ExplorerState<'a> {
    /// Which screen is showing.
    pub current_screen: ExplorerScreen,
    /// Set when the user asks to quit the explorer.
    pub should_quit: bool,

    /// The searchable list of catalog models.
    pub model_picker: ModelPicker<DataModel>,

    /// The display name of the model whose schema is loaded.
    pub target_model_name: String,
    /// The per-node schema detail facts, keyed by tree path.
    pub schema_details: HashMap<TreePath, SchemaNodeDetails>,
    /// The rendered schema tree.
    pub tree_items: Vec<TreeItem<'a, String>>,
    /// Which tree node is selected and which are expanded.
    pub tree_state: TreeState<String>,

    /// Set when a selected schema could not be turned into a tree, shown as an overlay.
    pub error_message: Option<String>,
}

impl<'a> ExplorerState<'a> {
    /// Starts the explorer on the model-selection screen listing `models`.
    #[must_use]
    pub fn new(models: Vec<DataModel>) -> ExplorerState<'a> {
        ExplorerState {
            current_screen: ExplorerScreen::ModelSelection,
            should_quit: false,
            model_picker: ModelPicker::new(models),
            target_model_name: String::new(),
            schema_details: HashMap::new(),
            tree_items: Vec::new(),
            tree_state: TreeState::default(),
            error_message: None,
        }
    }

    /// Loads a dereferenced schema into the viewer, building its node tree.
    ///
    /// # Errors
    /// Returns [`TuiError::Tree`] when two sibling nodes claim the same tree identifier.
    pub fn load_schema(&mut self, model: &DataModel, json: &Value, theme: &Theme) -> Result<()> {
        self.target_model_name = model.to_string();
        self.schema_details.clear();
        self.tree_items.clear();

        let root_name = json.get("title").and_then(|value| value.as_str()).unwrap_or("Root").to_string();

        let root_item = self.build_node_recursive(&root_name, json, &TreePath::new(), false, theme)?;
        self.tree_items = vec![root_item];

        self.tree_state.select(vec![root_name]);
        self.tree_state.open(Vec::new());
        self.current_screen = ExplorerScreen::SchemaViewer;

        Ok(())
    }

    fn build_node_recursive(&mut self, key: &str, schema: &Value, parent_path: &TreePath, is_required: bool, theme: &Theme) -> Result<TreeItem<'a, String>> {
        let current_path = parent_path.child(key.to_string());

        let raw_type = schema.get("type").and_then(|value| value.as_str());
        let raw_desc = schema.get("description").and_then(|value| value.as_str()).unwrap_or("");
        let format = schema.get("format").and_then(|value| value.as_str());

        let (display_type, icon, icon_color) = node_kind(key, raw_type, raw_desc, format, schema, theme);
        let description = clean_description(raw_desc);

        self.schema_details.insert(
            current_path.clone(),
            SchemaNodeDetails {
                description,
                data_type: display_type.to_string(),
                required: is_required,
                constraints: node_constraints(schema),
            },
        );

        let mut children: Vec<TreeItem<'a, String>> = Vec::new();

        if let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) {
            let requirements = required_fields(schema);
            for (child_key, child) in properties {
                let child_required = requirements.contains(&child_key.as_str());
                children.push(self.build_node_recursive(child_key, child, &current_path, child_required, theme)?);
            }
        }

        if let Some(all_of) = schema.get("allOf").and_then(|value| value.as_array()) {
            for sub in all_of {
                if let Some(properties) = sub.get("properties").and_then(|value| value.as_object()) {
                    let requirements = required_fields(sub);
                    for (child_key, child) in properties {
                        let child_required = requirements.contains(&child_key.as_str());
                        children.push(self.build_node_recursive(child_key, child, &current_path, child_required, theme)?);
                    }
                }
            }
        }

        for (field, label) in [("oneOf", "One Of"), ("anyOf", "Any Of")] {
            if let Some(options) = schema.get(field).and_then(|value| value.as_array()) {
                for (index, option) in options.iter().enumerate() {
                    let variant_name = format!("{} Option {}", label, index + 1);
                    children.push(self.build_node_recursive(&variant_name, option, &current_path, false, theme)?);
                }
            }
        }

        if let Some(items) = schema.get("items") {
            children.push(self.build_node_recursive("items", items, &current_path, false, theme)?);
        }

        children.sort_by(|first, second| first.identifier().cmp(second.identifier()));

        let display_span = Line::from(vec![
            Span::styled(format!("{icon:^3}"), Style::default().fg(icon_color)),
            Span::raw(" "),
            if is_required {
                Span::styled(key.to_string(), Style::default().fg(theme.key_required).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(key.to_string(), Style::default().fg(theme.text_primary))
            },
            if is_required {
                Span::styled("*", Style::default().fg(theme.key_required))
            } else {
                Span::raw("")
            },
        ]);

        TreeItem::new(key.to_string(), display_span, children).map_err(|error| TuiError::Tree(error.to_string()))
    }
}

/// Chooses the display label, icon glyph, and icon color for one schema node.
///
/// This is a display classifier: it distinguishes the NGSI-LD attribute kinds by their description
/// prefix and also labels plain JSON types and the mandatory `id`/`type` members, none of which the
/// attribute-kind classifier represents.
fn node_kind(key: &str, raw_type: Option<&str>, raw_desc: &str, format: Option<&str>, schema: &Value, theme: &Theme) -> (&'static str, &'static str, Color) {
    if key == "id" || key == "type" {
        return ("Identity", "@", Color::Rgb(230, 175, 50));
    }
    if raw_desc.starts_with("Relationship.") {
        return if raw_type == Some("array") {
            ("ListRelationship", "[&]", Color::Cyan)
        } else {
            ("Relationship", "->", Color::Cyan)
        };
    }
    if raw_desc.starts_with("GeoProperty.") {
        let geo_title = schema.get("title").and_then(|value| value.as_str()).unwrap_or("");
        return if geo_title.contains("LineString") || geo_title.contains("MultiLineString") {
            ("GeoProperty", "(~)", Color::Magenta)
        } else if geo_title.contains("Polygon") || geo_title.contains("MultiPolygon") {
            ("GeoProperty", "(<>)", Color::Magenta)
        } else {
            ("GeoProperty", "(+)", Color::Magenta)
        };
    }
    if raw_desc.starts_with("LanguageProperty.") {
        return ("LanguageProperty", "La", Color::Magenta);
    }
    if raw_desc.starts_with("VocabProperty.") {
        return ("VocabProperty", "(V)", Color::Magenta);
    }
    if raw_desc.starts_with("ListProperty.") {
        return ("ListProperty", "[]", Color::Blue);
    }
    if raw_desc.starts_with("JsonProperty.") {
        return ("JsonProperty", "{}", Color::Yellow);
    }

    match raw_type {
        Some("object") => ("Object", "{}", theme.folder_color),
        Some("array") => ("Array", "[]", Color::Blue),
        Some("number" | "integer") => ("Number", "#", Color::Yellow),
        Some("boolean") => ("Boolean", "?", Color::Yellow),
        Some("string") => match format {
            Some("date-time") => ("DateTime", "::", Color::Green),
            Some("date") => ("Date", "::", Color::Green),
            Some("time") => ("Time", "::", Color::Green),
            // `format` is an open string vocabulary; any other value is a plain string.
            _ => ("String", "Ab", Color::Green),
        },
        // `type` is an open string vocabulary; anything else is a mixed or complex node.
        _ => {
            if schema.get("oneOf").is_some() || schema.get("anyOf").is_some() {
                ("Mixed", "+", theme.folder_color)
            } else {
                ("Mixed/Complex", "*", theme.text_dim)
            }
        }
    }
}

/// Strips the NGSI-LD type prefixes from a description, leaving the human-readable text.
fn clean_description(raw_desc: &str) -> String {
    if raw_desc.is_empty() {
        return "No description provided.".to_string();
    }

    raw_desc
        .replace("Property. ", "")
        .replace("Relationship. ", "")
        .replace("GeoProperty. ", "")
        .replace("LanguageProperty. ", "")
        .replace("VocabProperty. ", "")
        .replace("ListProperty. ", "")
        .replace("JsonProperty. ", "")
        .replace("ListRelationship. ", "")
        .replace("Model:", "\nModel:")
}

/// The names a schema object marks as required.
fn required_fields(schema: &Value) -> Vec<&str> {
    schema
        .get("required")
        .and_then(|value| value.as_array())
        .map(|array| array.iter().filter_map(|value| value.as_str()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::{
        screen::explorer::{state::ExplorerState, tree_path::TreePath},
        theme::DARK,
    };
    use cassiopeia_ngsi_ld::data_model::DataModel;
    use serde_json::json;
    use std::str::FromStr;

    #[test]
    fn loading_a_schema_records_details_and_switches_to_the_viewer() {
        let mut state = ExplorerState::new(vec![DataModel::from_str("Sensor").unwrap()]);
        let schema = json!({
            "title": "Sensor",
            "required": ["temperature"],
            "properties": {"temperature": {"type": "number", "minimum": 0}},
        });

        state.load_schema(&DataModel::from_str("Sensor").unwrap(), &schema, &DARK).unwrap();

        assert_eq!(state.target_model_name, "Sensor");
        let details = &state.schema_details[&TreePath::from_segments(vec!["Sensor".to_string(), "temperature".to_string()])];
        assert!(details.required);
        assert_eq!(details.data_type, "Number");
    }
}
