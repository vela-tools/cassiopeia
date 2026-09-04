use crate::{
    screen::wizard::{editor_key::EditorKey, state::WizardState},
    theme::Theme,
};
use cassiopeia_mapping::transformation::Transformation;
use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use serde_json::Value;

/// Whether an attribute row is a leaf, a plain container, or a `oneOf` container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowKind {
    /// A value-carrying attribute with no children.
    Leaf,
    /// A structured-object container.
    Folder,
    /// A container whose children are the synthetic `oneOf` choices.
    OneOfParent,
}

/// The display facts about one attribute row, gathered so the list and the detail pane read the
/// tree once.
struct AttrRow {
    key: EditorKey,
    is_required: bool,
    has_source: bool,
    kind: RowKind,
    is_disabled: bool,
    attribute_type: NgsiLdAttributeKind,
    transformation: Option<Transformation>,
    source: Option<Value>,
}

/// Draws the attribute-list screen: the attribute list on the left, the detail pane on the right.
pub fn render(frame: &mut Frame, state: &mut WizardState, area: Rect, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let rows = build_rows(state);
    render_list(frame, state, &rows, chunks[0], theme);
    render_details(frame, state, &rows, chunks[1], theme);
}

/// Gathers the display facts for every attribute at the current depth.
fn build_rows(state: &WizardState) -> Vec<AttrRow> {
    state
        .get_current_sorted_keys()
        .iter()
        .filter_map(|key| {
            let map = state.get_current_map();
            let attribute = map.get(key)?;
            let kind = if attribute.is_container() {
                if attribute.mappings.keys().any(EditorKey::is_one_of_option) {
                    RowKind::OneOfParent
                } else {
                    RowKind::Folder
                }
            } else {
                RowKind::Leaf
            };

            Some(AttrRow {
                key: key.clone(),
                is_required: state.current_path.is_empty() && state.required_fields.contains(key.to_string().as_str()),
                has_source: attribute.source.is_some(),
                kind,
                is_disabled: state.is_attribute_disabled(key),
                attribute_type: attribute.attribute_type,
                transformation: attribute.transformation,
                source: attribute.source.clone(),
            })
        })
        .collect()
}

/// Draws the left-hand attribute list.
fn render_list(frame: &mut Frame, state: &mut WizardState, rows: &[AttrRow], area: Rect, theme: &Theme) {
    let items: Vec<ListItem> = rows.iter().map(|row| list_item(row, theme)).collect();

    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Attributes ")
                    .border_style(Style::default().fg(theme.accent)),
            )
            .highlight_style(Style::default().bg(theme.input_active).fg(Color::Black).add_modifier(Modifier::BOLD)),
        area,
        &mut state.attr_list_state,
    );
}

/// Builds one list row: a status dot, a type icon, the name, and required/disabled markers.
fn list_item<'a>(row: &AttrRow, theme: &Theme) -> ListItem<'a> {
    let (icon, type_color) = match row.kind {
        RowKind::OneOfParent => ("+", theme.folder_color),
        RowKind::Folder => ("{}", theme.folder_color),
        RowKind::Leaf => attr_icon(row.attribute_type, row.transformation),
    };

    let (status_char, status_color) = if row.has_source { ("●", theme.success) } else { ("○", theme.accent) };

    let name_style = if row.is_required {
        Style::default().fg(theme.key_required).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_primary)
    };
    let base_style = if row.is_disabled {
        Style::default().fg(theme.text_dim).add_modifier(Modifier::DIM)
    } else {
        name_style
    };

    let req_marker = if row.is_required { "*" } else { " " };
    let disabled_indicator = if row.is_disabled { " [x]" } else { "   " };

    ListItem::new(Line::from(vec![
        Span::styled(format!(" {status_char} "), Style::default().fg(status_color)),
        Span::styled(format!("{icon:^5}"), Style::default().fg(type_color)),
        Span::styled(format!("{:<20}", row.key), base_style),
        Span::styled(req_marker, Style::default().fg(theme.key_required)),
        Span::styled(disabled_indicator, Style::default().fg(theme.text_dim)),
    ]))
}

/// The icon glyph and color for a leaf attribute of a given type and conversion.
const fn attr_icon(attribute_type: NgsiLdAttributeKind, transformation: Option<Transformation>) -> (&'static str, Color) {
    match attribute_type {
        NgsiLdAttributeKind::Relationship => ("->", Color::Cyan),
        NgsiLdAttributeKind::ListRelationship => ("[&]", Color::Cyan),
        NgsiLdAttributeKind::LanguageProperty => ("La", Color::Magenta),
        NgsiLdAttributeKind::VocabProperty => ("(V)", Color::Magenta),
        NgsiLdAttributeKind::ListProperty => ("[]", Color::Blue),
        NgsiLdAttributeKind::JsonProperty => ("{}", Color::Yellow),
        NgsiLdAttributeKind::Property => match transformation {
            Some(Transformation::Integer | Transformation::Float) => ("#", Color::Yellow),
            Some(Transformation::Boolean) => ("?", Color::Yellow),
            Some(Transformation::Array) => ("[]", Color::Blue),
            Some(Transformation::DateTime | Transformation::Date | Transformation::Time) => ("::", Color::Green),
            Some(
                Transformation::String
                | Transformation::Object
                | Transformation::Geometry
                | Transformation::Point
                | Transformation::MultiPoint
                | Transformation::LineString
                | Transformation::MultiLineString
                | Transformation::Polygon
                | Transformation::MultiPolygon,
            )
            | None => ("Ab", Color::Green),
        },
        NgsiLdAttributeKind::GeoProperty => match transformation {
            Some(Transformation::LineString | Transformation::MultiLineString) => ("(~)", Color::Magenta),
            Some(Transformation::Polygon | Transformation::MultiPolygon) => ("(<>)", Color::Magenta),
            Some(
                Transformation::Geometry
                | Transformation::Point
                | Transformation::MultiPoint
                | Transformation::Boolean
                | Transformation::Integer
                | Transformation::Float
                | Transformation::String
                | Transformation::Array
                | Transformation::Object
                | Transformation::DateTime
                | Transformation::Date
                | Transformation::Time,
            )
            | None => ("(+)", Color::Magenta),
        },
    }
}

/// Draws the right-hand detail pane for the selected attribute.
fn render_details(frame: &mut Frame, state: &WizardState, rows: &[AttrRow], area: Rect, theme: &Theme) {
    let info_block = Block::default()
        .borders(Borders::ALL)
        .title(" Details ")
        .border_style(Style::default().fg(theme.border_secondary));

    let selected = state.attr_list_state.selected().and_then(|index| rows.get(index));
    let Some(row) = selected else {
        let help_text = vec![
            Line::from("No attributes."),
            Line::from(""),
            Line::from("[A]dd  [S]ave"),
            Line::from("[Esc/Backspace] go back"),
        ];
        frame.render_widget(Paragraph::new(help_text).block(info_block), area);
        return;
    };

    let schema_path = state.current_path.child(row.key.clone());

    let mut details = vec![
        Line::from(vec![
            Span::raw("Field: "),
            Span::styled(row.key.to_string(), Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
    ];

    if let Some(schema_details) = state.schema_details.get(&schema_path) {
        details.push(Line::from(vec![
            Span::raw("Schema Type: "),
            Span::styled(schema_details.data_type.clone(), Style::default().fg(theme.type_color)),
        ]));

        if schema_details.required {
            details.push(Line::from(vec![
                Span::raw("Status: "),
                Span::styled("[REQUIRED]", Style::default().fg(theme.key_required).add_modifier(Modifier::BOLD)),
            ]));
        }

        details.push(Line::from(""));
        details.push(Line::from(vec![Span::styled("Description", Style::default().add_modifier(Modifier::BOLD))]));
        details.push(Line::from(""));
        details.push(Line::from(schema_details.description.clone()));

        if !schema_details.constraints.is_empty() {
            details.push(Line::from(""));
            details.push(Line::from(vec![Span::styled("Constraints", Style::default().add_modifier(Modifier::BOLD))]));
            details.push(Line::from(""));
            for constraint in &schema_details.constraints {
                details.push(Line::from(vec![
                    Span::styled(format!("{}: ", constraint.kind), Style::default().fg(theme.text_dim)),
                    Span::styled(constraint.value.clone(), Style::default().fg(theme.key_optional)),
                ]));
            }
        }
    }

    details.push(Line::from(""));
    details.push(Line::from(vec![Span::styled("Wizard Info", Style::default().add_modifier(Modifier::BOLD))]));
    details.push(Line::from(""));
    details.push(Line::from(vec![
        Span::raw("Type:  "),
        Span::styled(format!("{:?}", row.attribute_type), Style::default().fg(theme.accent)),
    ]));
    details.push(Line::from(vec![
        Span::raw("Trans: "),
        Span::styled(format!("{:?}", row.transformation), Style::default().fg(theme.text_dim)),
    ]));

    if let Some(Value::String(source)) = &row.source {
        details.push(Line::from(vec![
            Span::raw("Source: "),
            Span::styled(source.clone(), Style::default().fg(theme.success)),
        ]));
    }

    details.push(Line::from(""));
    if row.kind != RowKind::Leaf {
        details.push(Line::from("Container with nested attributes. Press [Enter] to navigate."));
    } else if row.is_disabled {
        details.push(Line::from("Disabled - sibling option already mapped."));
    } else {
        details.push(Line::from("Press [Enter] or [E] to edit."));
    }

    frame.render_widget(Paragraph::new(details).block(info_block).wrap(Wrap { trim: true }), area);
}
