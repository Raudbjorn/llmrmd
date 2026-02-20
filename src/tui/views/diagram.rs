//! Diagram view: render and browse mermaid diagrams with live lint status.

use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::app::App;
use crate::tui::message::LintSeverity;

/// Top-level entry point for the diagram view. Splits the area into three
/// vertical zones: selector bar (3 rows), main content (flex), lint status
/// bar (3 rows).
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
            Constraint::Length(3), // Diagram selector bar
            Constraint::Min(1),   // Main content area (flex)
            Constraint::Length(3), // Lint status bar
        ])
        .split(area);

    render_selector(frame, chunks[0], app);
    render_diagram_content(frame, chunks[1], app);
    render_lint_bar(frame, chunks[2], app);
}

// ---------------------------------------------------------------------------
// Selector bar (top 3 rows)
// ---------------------------------------------------------------------------

fn render_selector(frame: &mut Frame, area: Rect, app: &App) {
    let has_search = !app.search_query.is_empty();

    // Build the main info line: "Diagram {idx+1}/{total}: {id} ({domain}) [{type}] {tok}tok"
    let current = app.diagrams.get(app.selected_diagram);
    let total = app.diagrams.len();

    let info_text = current
        .map(|d| {
            let pinned_marker = if app.pinned_diagrams.contains(&app.selected_diagram) {
                " [*]"
            } else {
                ""
            };
            format!(
                "Diagram {}/{}: {} ({}) [{}] {}tok{}",
                app.selected_diagram + 1,
                total,
                d.id,
                d.domain,
                d.diagram_type,
                d.tokens_est,
                pinned_marker,
            )
        })
        .unwrap_or_else(|| "No diagram selected".to_string());

    // Search match count, appended when relevant
    let search_note = if has_search && !app.search_results.is_empty() {
        format!("  ({} matches)", app.search_results.len())
    } else {
        String::new()
    };

    let nav_line = Line::from(vec![
        Span::styled(&info_text, Style::default().fg(Color::Cyan)),
        Span::styled(&search_note, Style::default().fg(Color::Magenta)),
        Span::raw("  "),
        Span::styled(
            "[h/l] navigate  [Space] pin  [e] edit  [r] repair",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let bar = Paragraph::new(nav_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Navigate Diagrams ")
            .title_style(Style::default().fg(Color::Yellow)),
    );

    frame.render_widget(bar, area);
}

// ---------------------------------------------------------------------------
// Main content area (flex middle)
// ---------------------------------------------------------------------------

/// Determine the border color based on lint severity:
///   - RED if any issue is `Error`
///   - YELLOW if only `Warning`s exist
///   - GREEN if clean
fn lint_border_color(app: &App) -> Color {
    if app.lint_issues.is_empty() {
        return Color::Green;
    }
    let has_error = app
        .lint_issues
        .iter()
        .any(|i| i.severity == LintSeverity::Error);
    if has_error {
        Color::Red
    } else {
        Color::Yellow
    }
}

fn render_diagram_content(frame: &mut Frame, area: Rect, app: &App) {
    let border_color = lint_border_color(app);

    // -- Edit mode -------------------------------------------------------
    if app.diagram_edit_mode {
        let editor = Paragraph::new(app.diagram_edit_buffer.as_str())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" Editing (Ctrl+S save, Esc cancel) ")
                    .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(editor, area);
        return;
    }

    // -- Normal view (with search filtering) -----------------------------
    let has_search = !app.search_query.is_empty() && !app.search_results.is_empty();
    let current_in_results = if has_search {
        app.search_results.contains(&app.selected_diagram)
    } else {
        true
    };

    let content = if !current_in_results {
        let matching_ids: Vec<String> = app
            .search_results
            .iter()
            .filter_map(|&idx| app.diagrams.get(idx))
            .map(|d| format!("  - {} [{}]", d.id, d.diagram_type))
            .collect();

        format!(
            "Current diagram does not match search \"{}\".\n\nMatching diagrams:\n{}",
            app.search_query,
            matching_ids.join("\n")
        )
    } else if app.rendered_diagram.is_empty() {
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
                .border_style(Style::default().fg(border_color))
                .title(format!(" Diagram -- {domain} [j/k:scroll] "))
                .title_style(Style::default().fg(Color::Yellow)),
        )
        .scroll((app.diagram_scroll, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(para, area);
}

// ---------------------------------------------------------------------------
// Lint status bar (bottom 3 rows)
// ---------------------------------------------------------------------------

fn render_lint_bar(frame: &mut Frame, area: Rect, app: &App) {
    let content: Line = if app.lint_issues.is_empty() {
        Line::from(Span::styled(
            "  No lint issues",
            Style::default().fg(Color::Green),
        ))
    } else {
        // Show first 3 issues inline; truncate the rest
        let max_display = 3;
        let mut spans: Vec<Span> = Vec::new();

        for (i, issue) in app.lint_issues.iter().take(max_display).enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            let (icon, color) = match issue.severity {
                LintSeverity::Warning => ("\u{26a0} ", Color::Yellow), // warning sign
                LintSeverity::Error => ("\u{2718} ", Color::Red),     // heavy ballot X
            };
            spans.push(Span::styled(icon, Style::default().fg(color)));
            spans.push(Span::styled(
                issue.message.as_str(),
                Style::default().fg(color),
            ));
        }

        let remaining = app.lint_issues.len().saturating_sub(max_display);
        if remaining > 0 {
            spans.push(Span::styled(
                format!("  (+{remaining} more)"),
                Style::default().fg(Color::DarkGray),
            ));
        }

        // Append the auto-heal hint when errors are present
        let has_errors = app
            .lint_issues
            .iter()
            .any(|i| i.severity == LintSeverity::Error);
        if has_errors {
            spans.push(Span::styled(
                "  [r] Auto-heal",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        Line::from(spans)
    };

    let issue_count = app.lint_issues.len();
    let title = if issue_count == 0 {
        " Lint ".to_string()
    } else {
        format!(" Lint ({issue_count}) ")
    };

    let bar_color = lint_border_color(app);
    let bar = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(bar_color))
            .title(title)
            .title_style(Style::default().fg(bar_color)),
    );

    frame.render_widget(bar, area);
}
