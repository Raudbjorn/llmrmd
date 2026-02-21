//! Plan view: phase-aware layout implementing the Plan-Verify-Execute gate.
//!
//! Renders one of three layouts depending on `app.plan_phase`:
//! - **Describe**: change description input, action hints, scope selector
//! - **Verify**: dual-pane plan reasoning + diagram preview with inline edit
//! - **Execute**: plan-approved confirmation with summary

use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::app::{App, PlanPhase};

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    match app.plan_phase {
        PlanPhase::Describe => render_describe(frame, area, app),
        PlanPhase::Verify => render_verify(frame, area, app),
        PlanPhase::Execute => render_execute(frame, area, app),
    }
}

// ---------------------------------------------------------------------------
// PlanPhase::Describe
// ---------------------------------------------------------------------------

fn render_describe(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Change description input
            Constraint::Length(3), // Action bar
            Constraint::Min(1),   // Scope list + diagrams
        ])
        .split(area);

    render_input(frame, chunks[0], app);
    render_actions(frame, chunks[1], app);
    render_scope_info(frame, chunks[2], app);
}

fn render_input(frame: &mut Frame, area: Rect, app: &App) {
    let cursor_char = if app.input_focused { "\u{258c}" } else { "" };
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
        let budget_label = format!(" [+/-] Budget: {} tokens ", app.token_budget);
        Line::from(vec![
            Span::styled(" e/Enter ", Style::default().fg(Color::Black).bg(Color::Yellow)),
            Span::raw(" Edit  "),
            Span::styled(" g ", Style::default().fg(Color::Black).bg(Color::Green)),
            Span::raw(" Generate  "),
            Span::styled(budget_label, Style::default().fg(Color::DarkGray)),
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

    // Inferred scopes with selection cursor
    let scope_items: Vec<ListItem> =
        if app.inferred_scopes.is_empty() && !app.change_description.is_empty() {
            // Preview: run scope inference without generating full plan
            let preview_scopes = crate::planner::scope::infer_scopes(
                &app.change_description,
                &app.manifest,
                &app.files,
            );
            preview_scopes
                .iter()
                .map(|s| {
                    ListItem::new(format!("  \u{2192} {s}"))
                        .style(Style::default().fg(Color::Yellow))
                })
                .collect()
        } else {
            app.inferred_scopes
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let is_selected = app.selected_scopes.contains(s);
                    let is_cursor = i == app.selected_scope;

                    let marker = if is_selected { "\u{2713}" } else { "\u{2022}" };
                    let cursor = if is_cursor { "\u{25b6}" } else { " " };
                    let label = format!("{cursor} {marker} {s}");

                    let style = if is_cursor {
                        Style::default()
                            .fg(if is_selected {
                                Color::Green
                            } else {
                                Color::Cyan
                            })
                            .add_modifier(Modifier::BOLD)
                            .bg(Color::DarkGray)
                    } else if is_selected {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    ListItem::new(label).style(style)
                })
                .collect()
        };

    let scope_title = if app.inferred_scopes.is_empty() {
        " Scopes (auto-inferred) ".to_string()
    } else {
        let selected_count = app
            .inferred_scopes
            .iter()
            .filter(|s| app.selected_scopes.contains(*s))
            .count();
        format!(
            " Scopes ({}/{}) [j/k:nav Space:toggle] ",
            selected_count,
            app.inferred_scopes.len()
        )
    };

    let scopes_list = List::new(scope_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(scope_title)
            .title_style(Style::default().fg(Color::Yellow)),
    );

    frame.render_widget(scopes_list, chunks[0]);

    // Diagram summary for matched scopes
    let relevant_diagrams: Vec<&crate::indexer::types::DiagramRecord> =
        if !app.inferred_scopes.is_empty() {
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
                " {} ({}, ~{} tok)",
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

// ---------------------------------------------------------------------------
// PlanPhase::Verify
// ---------------------------------------------------------------------------

fn render_verify(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),   // Dual-pane: reasoning + diagram preview
            Constraint::Length(3), // Action bar
        ])
        .split(area);

    // Split the main area into left/right 50/50
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    render_plan_reasoning(frame, panes[0], app);
    render_diagram_preview(frame, panes[1], app);
    render_verify_actions(frame, chunks[1], app);
}

fn render_plan_reasoning(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(ctx) = &app.planning_context {
        // Change description
        lines.push(Line::from(Span::styled(
            "Change:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        for desc_line in ctx.change_description.lines() {
            lines.push(Line::from(format!("  {desc_line}")));
        }
        lines.push(Line::from(""));

        // Relevant scopes
        lines.push(Line::from(Span::styled(
            "Relevant Scopes:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        if ctx.relevant_scopes.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (none)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for scope in &ctx.relevant_scopes {
                lines.push(Line::from(format!("  \u{2022} {scope}")));
            }
        }
        lines.push(Line::from(""));

        // Selected diagrams
        lines.push(Line::from(Span::styled(
            "Selected Diagrams:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        if ctx.selected_diagrams.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (none)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for (entry, _content) in &ctx.selected_diagrams {
                lines.push(Line::from(format!(
                    "  \u{2022} {} ({}, ~{} tok)",
                    entry.id, entry.diagram_type, entry.tokens_est
                )));
            }
        }
        lines.push(Line::from(""));

        // Total tokens
        lines.push(Line::from(vec![
            Span::styled(
                "Total Tokens: ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("~{}", ctx.total_tokens_est),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!(" / {} budget", app.token_budget),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "No plan generated yet.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Plan Reasoning ")
                .title_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

fn render_diagram_preview(frame: &mut Frame, area: Rect, app: &App) {
    if app.diagram_edit_mode {
        // Editable diagram buffer with cyan border
        let cursor_char = "\u{258c}";
        let text = format!("{}{cursor_char}", app.diagram_edit_buffer);

        let editor = Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" Mermaid IR Preview [editing] ")
                    .title_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(editor, area);
    } else {
        // Read-only diagram content
        let content = diagram_preview_content(app);

        let preview = Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" Mermaid IR Preview ")
                    .title_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(preview, area);
    }
}

/// Extract the best available diagram content for the preview pane.
fn diagram_preview_content(app: &App) -> String {
    // Prefer rendered_diagram if non-empty
    if !app.rendered_diagram.is_empty() {
        return app.rendered_diagram.clone();
    }

    // Fall back to first selected diagram from planning context
    if let Some(ctx) = &app.planning_context {
        if let Some((_entry, content)) = ctx.selected_diagrams.first() {
            return content.clone();
        }
        // Fall back to system diagram
        if !ctx.system_diagram.is_empty() {
            return ctx.system_diagram.clone();
        }
    }

    "(no diagram available)".to_string()
}

fn render_verify_actions(frame: &mut Frame, area: Rect, app: &App) {
    let actions = if app.diagram_edit_mode {
        Line::from(vec![
            Span::styled(" Esc ", Style::default().fg(Color::Black).bg(Color::Red)),
            Span::raw(" Cancel edit  "),
            Span::styled(
                " Ctrl+S ",
                Style::default().fg(Color::Black).bg(Color::Green),
            ),
            Span::raw(" Save  "),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                " Enter ",
                Style::default().fg(Color::Black).bg(Color::Green),
            ),
            Span::raw(" Approve  "),
            Span::styled(" r ", Style::default().fg(Color::Black).bg(Color::Red)),
            Span::raw(" Reject  "),
            Span::styled(
                " e ",
                Style::default().fg(Color::Black).bg(Color::Yellow),
            ),
            Span::raw(" Edit diagram  "),
            Span::styled(
                " Esc ",
                Style::default().fg(Color::Black).bg(Color::DarkGray),
            ),
            Span::raw(" Cancel edit"),
        ])
    };

    let bar = Paragraph::new(actions).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded),
    );

    frame.render_widget(bar, area);
}

// ---------------------------------------------------------------------------
// PlanPhase::Execute
// ---------------------------------------------------------------------------

fn render_execute(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Plan Approved",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    if let Some(ctx) = &app.planning_context {
        lines.push(Line::from(vec![
            Span::styled("  Scopes:   ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{}", ctx.relevant_scopes.len())),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Diagrams: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{}", ctx.selected_diagrams.len())),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Tokens:   ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("~{}", ctx.total_tokens_est)),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "  (no plan data)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            " 4 ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw(" Switch to Handoff to export"),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Execute ")
                .title_style(Style::default().fg(Color::Green)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}
