//! View rendering for each TUI screen.

use ratatui::prelude::*;
use ratatui::widgets::*;

use super::app::{App, View};

pub mod diagram;
pub mod handoff;
pub mod index;
pub mod plan;

/// Top-level render dispatcher.
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Layout: tabs at top, main content, status bar at bottom
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Tab bar
            Constraint::Min(1),    // Main content
            Constraint::Length(1), // Status bar
        ])
        .split(area);

    // Tab bar
    render_tabs(frame, chunks[0], app);

    // Main content (delegated to view-specific renderer)
    match app.current_view {
        View::Index => index::render(frame, chunks[1], app),
        View::Plan => plan::render(frame, chunks[1], app),
        View::Diagram => diagram::render(frame, chunks[1], app),
        View::Handoff => handoff::render(frame, chunks[1], app),
    }

    // Status bar
    render_status(frame, chunks[2], app);
}

fn render_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = vec![
        " 1:Index ".into(),
        " 2:Plan ".into(),
        " 3:Diagrams ".into(),
        " 4:Handoff ".into(),
    ];

    let selected = match app.current_view {
        View::Index => 0,
        View::Plan => 1,
        View::Diagram => 2,
        View::Handoff => 3,
    };

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" llmermaid 🏛️ "),
        )
        .select(selected)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(tabs, area);
}

fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    let mode_indicator = if app.input_focused {
        "INSERT"
    } else {
        match app.current_view {
            View::Index => "INDEX",
            View::Plan => "PLAN",
            View::Diagram => "DIAGRAM",
            View::Handoff => "HANDOFF",
        }
    };

    let status = Line::from(vec![
        Span::styled(
            format!(" {mode_indicator} "),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(&app.status_message, Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(
            " ?:help q:quit ",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    frame.render_widget(Paragraph::new(status), area);
}
