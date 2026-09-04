use crate::{
    components::{input::TextInput, popup::Popup},
    layout::centered_rect,
    screen::explorer::{
        state::{ExplorerScreen, ExplorerState},
        tree_path::TreePath,
    },
    theme::Theme,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, List, ListItem, Padding, Paragraph, Row, Scrollbar, Table, Wrap},
};
use tui_tree_widget::Tree;

/// Draws the whole explorer: header, the active screen, footer, and any error overlay.
pub fn ui(frame: &mut Frame, state: &mut ExplorerState, theme: &Theme) {
    frame.render_widget(Block::default().bg(theme.bg_main), frame.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(3)])
        .split(frame.area());

    render_header(frame, state, chunks[0], theme);

    match state.current_screen {
        ExplorerScreen::ModelSelection => render_selector(frame, state, chunks[1], theme),
        ExplorerScreen::SchemaViewer => render_viewer(frame, state, chunks[1], theme),
    }

    render_footer(frame, state, chunks[2], theme);

    if let Some(message) = &state.error_message {
        Popup::new("Error", message).with_footer("[Any key] Dismiss").render(frame, frame.area(), theme);
    }
}

fn render_header(frame: &mut Frame, state: &ExplorerState, area: Rect, theme: &Theme) {
    let title = match state.current_screen {
        ExplorerScreen::ModelSelection => "Model Selection".to_string(),
        ExplorerScreen::SchemaViewer => format!("Explorer: {}", state.target_model_name),
    };

    let header_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .bg(theme.bg_header);

    frame.render_widget(
        Paragraph::new(title)
            .block(header_block)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.text_header).add_modifier(Modifier::BOLD)),
        area,
    );
}

fn render_footer(frame: &mut Frame, state: &ExplorerState, area: Rect, theme: &Theme) {
    let help_spans = match state.current_screen {
        ExplorerScreen::ModelSelection => vec![
            Span::styled("[↑↓]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Navigate  "),
            Span::styled("[Enter]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Select  "),
            Span::styled("[Esc]", Style::default().fg(theme.error)),
            Span::raw(" Quit"),
        ],
        ExplorerScreen::SchemaViewer => vec![
            Span::styled("[Arrow Keys]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Navigate/Expand  "),
            Span::styled("[Enter]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Toggle  "),
            Span::styled("[Esc]", Style::default().fg(theme.border_secondary)),
            Span::raw(" Back  "),
        ],
    };

    let footer_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border_secondary))
        .bg(theme.bg_footer);

    frame.render_widget(Paragraph::new(Line::from(help_spans)).block(footer_block).alignment(Alignment::Center), area);
}

fn render_selector(frame: &mut Frame, state: &mut ExplorerState, area: Rect, theme: &Theme) {
    let popup_area = centered_rect(60, 60, area);

    let main_block = Block::default()
        .borders(Borders::ALL)
        .title(" Available Models ")
        .border_style(Style::default().fg(theme.border_secondary))
        .bg(theme.bg_surface);

    frame.render_widget(&main_block, popup_area);

    let inner_area = main_block.inner(popup_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner_area);

    TextInput::new("Search", &state.model_picker.search_query, true).render(frame, chunks[0], theme);

    let items: Vec<ListItem> = state
        .model_picker
        .filtered
        .iter()
        .map(|model| ListItem::new(format!(" {model} ")).style(Style::default().fg(theme.text_dim)))
        .collect();

    let list = List::new(items).highlight_style(Style::default().bg(theme.input_active).fg(Color::Black).add_modifier(Modifier::BOLD));

    frame.render_stateful_widget(list, chunks[1], &mut state.model_picker.list_state);
}

fn render_viewer(frame: &mut Frame, state: &mut ExplorerState, area: Rect, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let tree_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_secondary))
        .bg(theme.bg_surface)
        .title(Span::styled(" Schema Structure ", Style::default().fg(theme.accent)));

    match Tree::new(&state.tree_items) {
        Ok(tree) => {
            let widget = tree
                .block(tree_block)
                .experimental_scrollbar(Some(Scrollbar::default().thumb_symbol("║")))
                .highlight_style(Style::default().fg(Color::Black).bg(theme.input_active).add_modifier(Modifier::BOLD));
            frame.render_stateful_widget(widget, chunks[0], &mut state.tree_state);
        }
        Err(error) => {
            frame.render_widget(
                Paragraph::new(format!("Failed to render schema tree: {error}"))
                    .block(tree_block)
                    .style(Style::default().fg(theme.error)),
                chunks[0],
            );
        }
    }

    render_details(frame, state, chunks[1], theme);
}

fn render_details(frame: &mut Frame, state: &ExplorerState, area: Rect, theme: &Theme) {
    let lookup_key = TreePath::from_segments(state.tree_state.selected().iter().map(ToString::to_string).collect());

    let details_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_secondary))
        .bg(theme.bg_surface)
        .title(" Property Details ")
        .padding(Padding::new(2, 2, 1, 1));

    let Some(details) = state.schema_details.get(&lookup_key) else {
        frame.render_widget(
            Paragraph::new("Select a property to view details.")
                .style(Style::default().fg(theme.text_dim))
                .block(details_block),
            area,
        );
        return;
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(8), Constraint::Min(5)])
        .split(details_block.inner(area));

    frame.render_widget(details_block, area);

    // The node's own key is the last segment of its path; the detail record no longer stores it.
    let field_name = lookup_key.last().unwrap_or_default().to_string();
    let mut header_spans = vec![
        Span::raw("Field: "),
        Span::styled(field_name, Style::default().add_modifier(Modifier::BOLD).fg(theme.text_header)),
        Span::raw("   Type: "),
        Span::styled(&details.data_type, Style::default().fg(theme.type_color)),
    ];

    if details.required {
        header_spans.push(Span::raw("   "));
        header_spans.push(Span::styled("[REQUIRED]", Style::default().fg(theme.key_required).add_modifier(Modifier::BOLD)));
    }

    let header = Paragraph::new(Line::from(header_spans)).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(theme.border_secondary)),
    );
    frame.render_widget(header, layout[0]);

    let description = Paragraph::new(details.description.clone())
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(theme.text_primary))
        .block(Block::default().padding(Padding::vertical(1)));
    frame.render_widget(description, layout[1]);

    if !details.constraints.is_empty() {
        let rows: Vec<Row> = details
            .constraints
            .iter()
            .map(|constraint| {
                Row::new(vec![
                    Cell::from(constraint.kind.to_string()).style(Style::default().fg(theme.text_dim)),
                    Cell::from(constraint.value.as_str()).style(Style::default().fg(theme.key_optional)),
                ])
            })
            .collect();

        let table = Table::new(rows, [Constraint::Length(15), Constraint::Min(10)]).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(theme.border_secondary))
                .title(" Constraints "),
        );
        frame.render_widget(table, layout[2]);
    }
}
