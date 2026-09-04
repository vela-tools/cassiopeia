use crate::{
    layout::centered_rect,
    screen::wizard::{attribute_editor::AttributeEditorState, state::WizardState, ui::field::Field},
    theme::Theme,
};
use cassiopeia_mapping::transformation::Transformation;
use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// Draws the attribute editor: the name, type, transformation, source, and (for relationships)
/// target-entity fields.
pub fn render(frame: &mut Frame, state: &WizardState, area: Rect, theme: &Theme) {
    let form_area = centered_rect(60, 80, area);

    let form_block = Block::default()
        .borders(Borders::ALL)
        .title(" Attribute Editor ")
        .border_style(Style::default().fg(theme.border_secondary))
        .bg(theme.bg_surface);

    frame.render_widget(&form_block, form_area);

    let inner_area = form_block.inner(form_area);
    let chunks = Layout::default()
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(inner_area);

    let editor = &state.attribute_editor;
    let type_label = editor.type_label();
    let transform_label = editor.transform_label();

    Field {
        title: "Name",
        content: &editor.name,
        field_idx: 0,
        focus_idx: editor.focus_index,
        is_disabled: editor.is_schema_derived,
    }
    .render(frame, chunks[0], theme);

    Field {
        title: "Type",
        content: &type_label,
        field_idx: 1,
        focus_idx: editor.focus_index,
        is_disabled: editor.is_schema_derived,
    }
    .render(frame, chunks[1], theme);

    Field {
        title: "Transformation",
        content: &transform_label,
        field_idx: 2,
        focus_idx: editor.focus_index,
        is_disabled: editor.is_schema_derived && !transform_is_editable(editor),
    }
    .render(frame, chunks[2], theme);

    render_source_field(frame, editor, chunks[3], theme);

    if matches!(
        AttributeEditorState::TYPES[editor.attr_type_idx],
        NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship
    ) {
        Field {
            title: "Target Entity",
            content: &editor.target_entity,
            field_idx: 4,
            focus_idx: editor.focus_index,
            is_disabled: false,
        }
        .render(frame, chunks[4], theme);
    }
}

/// Whether the transformation field is editable despite being schema-derived: only a numeric
/// property may still switch between integer and float.
fn transform_is_editable(editor: &AttributeEditorState) -> bool {
    editor.is_schema_derived
        && matches!(
            AttributeEditorState::TYPES[editor.attr_type_idx],
            NgsiLdAttributeKind::Property | NgsiLdAttributeKind::LanguageProperty
        )
        && matches!(
            AttributeEditorState::TRANSFORMS.get(editor.transform_idx.saturating_sub(1)),
            Some((_, Transformation::Integer | Transformation::Float))
        )
}

/// Draws the source-template field: the committed tags followed by the in-progress input.
fn render_source_field(frame: &mut Frame, editor: &AttributeEditorState, area: Rect, theme: &Theme) {
    let is_focused = editor.focus_index == 3;
    let title = if is_focused { "> Source Template " } else { "  Source Template " };
    let style = if editor.is_container_mode {
        Style::default().fg(theme.locked).add_modifier(Modifier::DIM)
    } else if is_focused {
        Style::default().fg(theme.input_active).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.border_secondary)
    };

    let mut spans = Vec::new();
    for tag in &editor.source_parts {
        spans.push(Span::styled(format!(" {tag} "), Style::default().bg(theme.tag_bg).fg(theme.tag_text)));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        editor.source_input.clone(),
        Style::default().fg(if is_focused { theme.text_primary } else { theme.text_dim }),
    ));

    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_style(style)
                .border_style(style)
                .bg(theme.bg_surface),
        ),
        area,
    );
}
