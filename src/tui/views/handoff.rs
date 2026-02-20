//! Handoff view: review and export the generated planning context,
//! with an optional agent execution pipeline visualizer.

use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::agent::executor::AgentStepStatus;
use crate::planner;
use crate::tui::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    match &app.planning_context {
        Some(ctx) => render_context(frame, area, app, ctx),
        None => {
            let empty = Paragraph::new(
                "No planning context generated yet.\nGo to Plan (2) \u{2192} describe a change \u{2192} press Enter.",
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" Handoff ")
                    .title_style(Style::default().fg(Color::Yellow)),
            )
            .style(Style::default().fg(Color::DarkGray));

            frame.render_widget(empty, area);
        }
    }
}

fn render_context(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    ctx: &crate::planner::types::PlannerContext,
) {
    let has_pipeline = app.graph_executor.is_some();

    let mut constraints = vec![Constraint::Length(5)]; // Summary

    if has_pipeline {
        constraints.push(Constraint::Length(6)); // Agent pipeline (3 lines + border)
    }

    constraints.push(Constraint::Min(1));    // Prompt preview (flex)
    constraints.push(Constraint::Length(3));  // Action bar

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut chunk_idx = 0;

    // -- Summary panel --
    render_summary(frame, chunks[chunk_idx], ctx);
    chunk_idx += 1;

    // -- Agent pipeline (conditional) --
    if has_pipeline {
        render_agent_pipeline(frame, chunks[chunk_idx], app);
        chunk_idx += 1;
    }

    // -- Prompt preview --
    render_prompt_preview(frame, chunks[chunk_idx], app, ctx);
    chunk_idx += 1;

    // -- Action bar --
    render_action_bar(frame, chunks[chunk_idx]);
}

/// Summary panel: change description, scopes, diagram/token counts.
fn render_summary(
    frame: &mut Frame,
    area: Rect,
    ctx: &crate::planner::types::PlannerContext,
) {
    let summary = vec![
        Line::from(vec![
            Span::styled(" Change: ", Style::default().fg(Color::Yellow)),
            Span::raw(&ctx.change_description),
        ]),
        Line::from(vec![
            Span::styled(" Scopes: ", Style::default().fg(Color::Yellow)),
            Span::raw(ctx.relevant_scopes.join(", ")),
        ]),
        Line::from(vec![
            Span::styled(" Diagrams: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{}", ctx.selected_diagrams.len())),
            Span::raw("  "),
            Span::styled(" CLAUDE.md: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{}", ctx.relevant_claude_mds.len())),
            Span::raw("  "),
            Span::styled(" ~Tokens: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{}", ctx.total_tokens_est)),
        ]),
    ];

    let summary_widget = Paragraph::new(summary).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Planning Summary ")
            .title_style(Style::default().fg(Color::Yellow)),
    );

    frame.render_widget(summary_widget, area);
}

/// Horizontal agent pipeline: [Step1] --> [Step2] --> [Step3]
/// Color-coded by AgentStepStatus, driven by the real GraphExecutor.
fn render_agent_pipeline(frame: &mut Frame, area: Rect, app: &App) {
    let steps = app.agent_display_steps();
    let (done, total) = app
        .graph_executor
        .as_ref()
        .map(|e| e.progress())
        .unwrap_or((0, 0));

    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::raw(" "));

    for (i, step) in steps.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" --> ", Style::default().fg(Color::DarkGray)));
        }

        let (fg, modifier) = match &step.status {
            AgentStepStatus::Pending => (Color::DarkGray, Modifier::empty()),
            AgentStepStatus::Active => (Color::Yellow, Modifier::BOLD),
            AgentStepStatus::Completed => (Color::Green, Modifier::empty()),
            AgentStepStatus::Failed(_) => (Color::Red, Modifier::BOLD),
        };

        let bracket_style = Style::default().fg(fg).add_modifier(modifier);
        let label_style = Style::default().fg(fg).add_modifier(modifier);

        spans.push(Span::styled("[", bracket_style));
        spans.push(Span::styled(step.label.clone(), label_style));
        spans.push(Span::styled("]", bracket_style));
    }

    let pipeline_line = Line::from(spans);

    // Build a second line: active step label or completion status + progress fraction
    let active_label: String = steps
        .iter()
        .find(|s| s.status == AgentStepStatus::Active)
        .map(|s| s.label.clone())
        .unwrap_or_default();

    let status_line = if app.has_pending_gate() {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "Waiting for approval -- press Enter to approve, s to skip",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else if !active_label.is_empty() {
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Active: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                active_label,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  ({done}/{total})")),
        ])
    } else if done == total && total > 0 {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("All steps complete ({done}/{total})"),
                Style::default().fg(Color::Green),
            ),
        ])
    } else {
        Line::from(format!("  ({done}/{total})"))
    };

    // Third line: show truncated output from the most recently completed node
    let last_output_line = app
        .graph_executor
        .as_ref()
        .and_then(|e| {
            e.state()
                .node_states
                .iter()
                .rev()
                .find(|s| s.output.is_some())
                .and_then(|s| s.output.as_ref())
                .map(|o| {
                    let truncated = if o.len() > 80 {
                        format!("  > {}...", &o[..77])
                    } else {
                        format!("  > {o}")
                    };
                    Line::from(Span::styled(truncated, Style::default().fg(Color::DarkGray)))
                })
        })
        .unwrap_or_else(|| Line::from(""));

    let content = vec![pipeline_line, status_line, last_output_line];

    let pipeline_widget = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(format!(" Agent Pipeline ({done}/{total}) "))
            .title_style(Style::default().fg(Color::Cyan)),
    );

    frame.render_widget(pipeline_widget, area);
}

/// Scrollable prompt preview.
fn render_prompt_preview(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    ctx: &crate::planner::types::PlannerContext,
) {
    let prompt = planner::render_planning_prompt(ctx);
    let preview = Paragraph::new(prompt)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Generated Prompt [j/k:scroll] ")
                .title_style(Style::default().fg(Color::Yellow)),
        )
        .scroll((app.handoff_scroll, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(preview, area);
}

/// Action bar with keybinding hints.
fn render_action_bar(frame: &mut Frame, area: Rect) {
    let actions = Line::from(vec![
        Span::styled(" x ", Style::default().fg(Color::Black).bg(Color::Green)),
        Span::raw(" Export  "),
        Span::styled(" a ", Style::default().fg(Color::Black).bg(Color::Cyan)),
        Span::raw(" Start Pipeline  "),
        Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::Yellow)),
        Span::raw(" Approve Gate  "),
        Span::styled(" s ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
        Span::raw(" Skip  "),
        Span::styled(" j/k ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
        Span::raw(" Scroll"),
    ]);

    let action_bar = Paragraph::new(actions).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded),
    );

    frame.render_widget(action_bar, area);
}
