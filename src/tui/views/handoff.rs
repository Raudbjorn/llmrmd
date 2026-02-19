//! Handoff view: review and export the generated planning context.

use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::planner;
use crate::tui::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    match &app.planning_context {
        Some(ctx) => render_context(frame, area, app, ctx),
        None => {
            let empty = Paragraph::new(
                "No planning context generated yet.\nGo to Plan (2) → describe a change → press Enter.",
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Summary
            Constraint::Min(1),   // Full prompt preview
            Constraint::Length(3), // Actions
        ])
        .split(area);

    // Summary
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

    frame.render_widget(summary_widget, chunks[0]);

    // Full prompt preview
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

    frame.render_widget(preview, chunks[1]);

    // Actions
    let actions = Line::from(vec![
        Span::styled(" x ", Style::default().fg(Color::Black).bg(Color::Green)),
        Span::raw(" Export to .claude/planner-context.md  "),
        Span::styled(" 2 ", Style::default().fg(Color::Black).bg(Color::Yellow)),
        Span::raw(" Back to Plan  "),
    ]);

    let action_bar = Paragraph::new(actions).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded),
    );

    frame.render_widget(action_bar, chunks[2]);
}
