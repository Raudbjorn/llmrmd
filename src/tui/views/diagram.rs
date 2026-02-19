//! Diagram view: render and browse mermaid diagrams.

use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    if app.diagrams.is_empty() {
        let empty = Paragraph::new("No diagrams found. Run the indexer first (press 'i').")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" Diagrams ")
                    .title_style(Style::default().fg(Color::Yellow)),
            )
            .style(Style::default().fg(Color::DarkGray));

        frame.render_widget(empty, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Diagram selector
            Constraint::Min(1),   // Rendered diagram
        ])
        .split(area);

    render_selector(frame, chunks[0], app);
    render_diagram_content(frame, chunks[1], app);
}

fn render_selector(frame: &mut Frame, area: Rect, app: &App) {
    let current = app.diagrams.get(app.selected_diagram);
    let label = current
        .map(|d| {
            format!(
                " {}/{} — {} [{}] ~{} tokens ",
                app.selected_diagram + 1,
                app.diagrams.len(),
                d.id,
                d.diagram_type,
                d.tokens_est,
            )
        })
        .unwrap_or_else(|| " No diagram selected ".to_string());

    let nav = Line::from(vec![
        Span::styled(" ← h ", Style::default().fg(Color::Black).bg(Color::Yellow)),
        Span::raw(" prev  "),
        Span::styled(" l → ", Style::default().fg(Color::Black).bg(Color::Yellow)),
        Span::raw(" next  "),
        Span::styled(&label, Style::default().fg(Color::Cyan)),
    ]);

    let bar = Paragraph::new(nav).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Navigate Diagrams ")
            .title_style(Style::default().fg(Color::Yellow)),
    );

    frame.render_widget(bar, area);
}

fn render_diagram_content(frame: &mut Frame, area: Rect, app: &App) {
    let content = if app.rendered_diagram.is_empty() {
        app.diagrams
            .get(app.selected_diagram)
            .map(|d| format!("(Press any arrow key to render)\n\n{}", d.content))
            .unwrap_or_default()
    } else {
        app.rendered_diagram.clone()
    };

    let domain = app
        .diagrams
        .get(app.selected_diagram)
        .map(|d| d.domain.as_str())
        .unwrap_or("unknown");

    let para = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(format!(" Diagram — {domain} [j/k:scroll] "))
                .title_style(Style::default().fg(Color::Yellow)),
        )
        .scroll((app.diagram_scroll, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(para, area);
}
