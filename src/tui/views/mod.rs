//! View rendering for each TUI screen.

use ratatui::prelude::*;
use ratatui::widgets::*;

use super::app::{App, View};
use super::message::StatusLevel;

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

    // Help overlay (drawn last so it renders on top)
    if app.show_help {
        render_help_overlay(frame, area, app);
    }

    // Fuzzy finder overlay (drawn after help so it takes precedence)
    if app.fuzzy_open {
        render_fuzzy_overlay(frame, area, app);
    }
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
                .title(" llmermaid "),
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
    } else if app.search_active {
        "SEARCH"
    } else {
        match app.current_view {
            View::Index => "INDEX",
            View::Plan => "PLAN",
            View::Diagram => "DIAGRAM",
            View::Handoff => "HANDOFF",
        }
    };

    let mode_bg = if app.input_focused || app.search_active {
        Color::Yellow
    } else {
        Color::Cyan
    };

    let status_color = match app.status_level {
        StatusLevel::Info => Color::DarkGray,
        StatusLevel::Success => Color::Green,
        StatusLevel::Warning => Color::Yellow,
        StatusLevel::Error => Color::Red,
    };

    let spinner = if app.is_busy {
        let frames = ['|', '/', '-', '\\', '|', '/', '-', '\\', '.', '.'];
        let ch = frames[app.spinner_frame % frames.len()];
        format!(" {ch} ")
    } else {
        String::new()
    };

    let search_display = if app.search_active {
        format!(" /{} ", app.search_query)
    } else if !app.search_query.is_empty() {
        // Search submitted but query retained: show query + match count
        let n = app.search_results.len();
        format!(" /{} ({n} matches) ", app.search_query)
    } else {
        String::new()
    };

    let status = Line::from(vec![
        Span::styled(
            format!(" {mode_indicator} "),
            Style::default()
                .fg(Color::Black)
                .bg(mode_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(&spinner),
        Span::styled(
            &search_display,
            Style::default().fg(if app.search_active {
                Color::Yellow
            } else {
                Color::Cyan
            }),
        ),
        Span::raw(" "),
        Span::styled(&app.status_message, Style::default().fg(status_color)),
        Span::raw("  "),
        Span::styled(
            " ?:help q:quit ",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    frame.render_widget(Paragraph::new(status), area);
}

/// Render a centered help overlay with keybindings for the current view.
fn render_help_overlay(frame: &mut Frame, area: Rect, app: &App) {
    // Size the popup: ~60 cols wide, height depends on content
    let popup_width = 60u16.min(area.width.saturating_sub(4));
    let popup_height = 24u16.min(area.height.saturating_sub(4));

    let popup_area = centered_rect(popup_width, popup_height, area);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    // Build help text: global keys first
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " Global Keybindings ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        help_line("q / Ctrl+C", "Quit"),
        help_line("1-4", "Switch view (Index/Plan/Diagram/Handoff)"),
        help_line("i", "Run indexer"),
        help_line("?", "Toggle this help"),
        help_line("/", "Start search"),
        help_line("+/-", "Increase/decrease token budget"),
        help_line("Ctrl+E", "Open external editor"),
        help_line("Ctrl+F", "Open fuzzy finder"),
        Line::from(""),
    ];

    // View-specific keys
    let (view_title, view_keys) = match app.current_view {
        View::Index => (
            "Index View",
            vec![
                ("j/k or Up/Down", "Navigate files"),
                ("h/l or Left/Right", "Navigate domains"),
                ("Tab", "Toggle all/filtered files"),
            ],
        ),
        View::Plan => (
            "Plan View",
            vec![
                ("e / Enter", "Edit change description"),
                ("g", "Generate plan"),
                ("j/k", "Navigate scopes"),
                ("Space", "Toggle scope"),
                ("Enter", "Approve plan (Verify phase)"),
                ("r", "Reject plan (Verify phase)"),
                ("Esc", "Stop editing (in input mode)"),
            ],
        ),
        View::Diagram => (
            "Diagram View",
            vec![
                ("h/l or Left/Right", "Previous/next diagram"),
                ("j/k or Up/Down", "Scroll diagram"),
                ("Home", "Scroll to top"),
                ("Space", "Toggle pin diagram"),
                ("r", "Run linter"),
                ("e", "Edit diagram inline"),
            ],
        ),
        View::Handoff => (
            "Handoff View",
            vec![
                ("j/k or Up/Down", "Scroll prompt preview"),
                ("Home", "Scroll to top"),
                ("x", "Export to .claude/planner-context.md"),
                ("a", "Start agent pipeline"),
                ("Enter", "Approve gate"),
                ("s", "Skip current step"),
            ],
        ),
    };

    lines.push(Line::from(Span::styled(
        format!(" {view_title} "),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    for (key, desc) in view_keys {
        lines.push(help_line(key, desc));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Press any key to dismiss ",
        Style::default().fg(Color::DarkGray),
    )));

    let help = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" Help ")
                .title_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .alignment(Alignment::Left);

    frame.render_widget(help, popup_area);
}

/// Render the structural fuzzy finder overlay.
fn render_fuzzy_overlay(frame: &mut Frame, area: Rect, app: &App) {
    // 80% width, 70% height, centered
    let popup_width = (area.width * 80 / 100).max(40);
    let popup_height = (area.height * 70 / 100).max(12);
    let popup_area = centered_rect(popup_width, popup_height, area);

    // Clear behind
    frame.render_widget(Clear, popup_area);

    // Outer block with Double border in Cyan
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Fuzzy Finder ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    let inner_area = outer_block.inner(popup_area);
    frame.render_widget(outer_block, popup_area);

    // Vertical layout: search input (3), results+preview (flex), hints (1)
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Search input
            Constraint::Min(1),   // Results + Preview
            Constraint::Length(1), // Bottom hints
        ])
        .split(inner_area);

    // -- Search input --
    let cursor_char = if app.spinner_frame % 2 == 0 { "_" } else { " " };
    let search_text = format!("/ {}{}", app.fuzzy_query, cursor_char);
    let search_input = Paragraph::new(search_text)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Search ")
                .title_style(Style::default().fg(Color::Cyan)),
        );
    frame.render_widget(search_input, v_chunks[0]);

    // -- Results (left 60%) + Preview (right 40%) --
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(v_chunks[1]);

    // Left pane: results list
    render_fuzzy_results(frame, h_chunks[0], app);

    // Right pane: preview of selected result
    render_fuzzy_preview(frame, h_chunks[1], app);

    // -- Bottom hints --
    let hints = Line::from(vec![
        Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::Cyan)),
        Span::raw(" Pin/Unpin  "),
        Span::styled(" Esc ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
        Span::raw(" Close  "),
        Span::styled(" Up/Down ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
        Span::raw(" Navigate"),
    ]);
    frame.render_widget(Paragraph::new(hints), v_chunks[2]);
}

/// Render the fuzzy results list (left pane).
fn render_fuzzy_results(frame: &mut Frame, area: Rect, app: &App) {
    let results_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(format!(" Results ({}) ", app.fuzzy_results.len()))
        .title_style(Style::default().fg(Color::White));

    if app.fuzzy_results.is_empty() {
        let empty_msg = if app.fuzzy_query.is_empty() {
            "Type to search diagrams..."
        } else {
            "No matches"
        };
        let empty = Paragraph::new(empty_msg)
            .style(Style::default().fg(Color::DarkGray))
            .block(results_block);
        frame.render_widget(empty, area);
        return;
    }

    let inner_height = results_block.inner(area).height as usize;

    // Compute visible window around selected item
    let total = app.fuzzy_results.len();
    let selected = app.fuzzy_selected.min(total.saturating_sub(1));
    let start = if selected >= inner_height {
        selected - inner_height + 1
    } else {
        0
    };

    let items: Vec<ListItem> = app
        .fuzzy_results
        .iter()
        .enumerate()
        .skip(start)
        .take(inner_height)
        .map(|(i, r)| {
            let pinned = if app.pinned_diagrams.contains(&r.diagram_idx) {
                "*"
            } else {
                " "
            };

            // Left side: pinned marker + diagram info
            let label = format!(
                "{} {} ({}) [{}] {}tok",
                pinned, r.diagram_id, r.domain, r.diagram_type, r.tokens_est
            );

            // Right side: relevance score
            let score = format!(" {:.1}", r.relevance);

            let style = if i == selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let line = Line::from(vec![
                Span::styled(label, style),
                Span::styled(score, Style::default().fg(Color::DarkGray)),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(results_block);
    frame.render_widget(list, area);
}

/// Render the preview of the selected fuzzy result (right pane).
fn render_fuzzy_preview(frame: &mut Frame, area: Rect, app: &App) {
    let preview_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Preview ")
        .title_style(Style::default().fg(Color::White));

    let content = app
        .fuzzy_results
        .get(app.fuzzy_selected)
        .map(|r| r.content_preview.as_str())
        .unwrap_or("");

    let preview = Paragraph::new(content)
        .style(Style::default().fg(Color::Gray))
        .block(preview_block)
        .wrap(Wrap { trim: false });

    frame.render_widget(preview, area);
}

/// Format a single help line: key binding on the left, description on the right.
fn help_line<'a>(key: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("{key:<20}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc, Style::default().fg(Color::White)),
    ])
}

/// Calculate a centered `Rect` within a given area.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
