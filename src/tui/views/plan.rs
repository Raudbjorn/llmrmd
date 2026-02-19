//! Plan view: describe a change and select scopes.

use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Change description input
            Constraint::Length(3),  // Action bar
            Constraint::Min(1),    // Scope list + info
        ])
        .split(area);

    render_input(frame, chunks[0], app);
    render_actions(frame, chunks[1], app);
    render_scope_info(frame, chunks[2], app);
}

fn render_input(frame: &mut Frame, area: Rect, app: &App) {
    let cursor_char = if app.input_focused { "▌" } else { "" };
    let text = format!("{}{cursor_char}", app.change_description);

    let border_style = if app.input_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Gray)
    };

    let input = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(border_style)
                .title(" Change Description [e:edit Enter:generate] ")
                .title_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(input, area);
}

fn render_actions(frame: &mut Frame, area: Rect, app: &App) {
    let actions = if app.change_description.is_empty() {
        Line::from(vec![
            Span::styled(" e ", Style::default().fg(Color::Black).bg(Color::Yellow)),
            Span::raw(" Describe change  "),
        ])
    } else {
        let budget_label = format!(" Budget: {} tokens ", app.token_budget);
        Line::from(vec![
            Span::styled(" e ", Style::default().fg(Color::Black).bg(Color::Yellow)),
            Span::raw(" Edit  "),
            Span::styled(" g ", Style::default().fg(Color::Black).bg(Color::Green)),
            Span::raw(" Generate plan  "),
            Span::styled(
                budget_label,
                Style::default().fg(Color::DarkGray),
            ),
        ])
    };

    let bar = Paragraph::new(actions).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded),
    );

    frame.render_widget(bar, area);
}

fn render_scope_info(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Inferred scopes
    let scope_items: Vec<ListItem> = if app.inferred_scopes.is_empty() && !app.change_description.is_empty() {
        // Preview: run scope inference without generating full plan
        let preview_scopes = crate::planner::scope::infer_scopes(
            &app.change_description,
            &app.manifest,
            &app.files,
        );
        preview_scopes
            .iter()
            .map(|s| {
                ListItem::new(format!("  → {s}"))
                    .style(Style::default().fg(Color::Yellow))
            })
            .collect()
    } else {
        app.inferred_scopes
            .iter()
            .map(|s| {
                let checked = if app.selected_scopes.contains(s) {
                    "✓"
                } else {
                    "•"
                };
                ListItem::new(format!(" {checked} {s}"))
                    .style(Style::default().fg(Color::Green))
            })
            .collect()
    };

    let scopes_list = List::new(scope_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Scopes (auto-inferred) ")
            .title_style(Style::default().fg(Color::Yellow)),
    );

    frame.render_widget(scopes_list, chunks[0]);

    // Diagram summary for matched scopes
    let relevant_diagrams: Vec<&crate::indexer::types::DiagramRecord> = if !app.inferred_scopes.is_empty() {
        app.diagrams
            .iter()
            .filter(|d| app.inferred_scopes.contains(&d.domain) || d.domain == "root")
            .collect()
    } else {
        Vec::new()
    };

    let diag_items: Vec<ListItem> = relevant_diagrams
        .iter()
        .map(|d| {
            ListItem::new(format!(
                " 📊 {} ({}, ~{} tok)",
                d.id, d.diagram_type, d.tokens_est
            ))
            .style(Style::default().fg(Color::Magenta))
        })
        .collect();

    let diag_list = List::new(diag_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Matching Diagrams ")
            .title_style(Style::default().fg(Color::Yellow)),
    );

    frame.render_widget(diag_list, chunks[1]);
}
