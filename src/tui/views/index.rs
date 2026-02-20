//! Index view: three-panel layout with domains, files, and diagram summary
//! with token budget gauge.

use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::app::App;
use crate::tui::widgets::TokenGauge;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    // Three-panel horizontal split: domains 20%, files 50%, diagrams 30%
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(50),
            Constraint::Percentage(30),
        ])
        .split(area);

    render_domains(frame, chunks[0], app);
    render_files(frame, chunks[1], app);
    render_diagram_summary(frame, chunks[2], app);
}

fn render_domains(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .domains
        .iter()
        .enumerate()
        .map(|(i, domain)| {
            let file_count = app.files.iter().filter(|f| f.domain == *domain).count();
            let diag_count = app.diagrams.iter().filter(|d| d.domain == *domain).count();

            // Show boundary type next to domain name when known
            let boundary_suffix = app
                .boundaries
                .iter()
                .find(|b| b.domain == *domain)
                .map(|b| format!(" [{}]", b.boundary_type.as_str()))
                .unwrap_or_default();

            let mut line = format!("{domain}{boundary_suffix}  ({file_count}");
            if diag_count > 0 {
                line.push_str(&format!(", {diag_count} diag"));
            }
            line.push(')');

            let style = if i == app.selected_domain {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(format!(" Domains ({}) ", app.domains.len()))
                .title_style(Style::default().fg(Color::Yellow)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(list, area);
}

fn render_files(frame: &mut Frame, area: Rect, app: &App) {
    let filtered = app.filtered_files();

    // If a search query is active, narrow to matching files only
    let has_search = !app.search_query.is_empty();
    let query_lower = app.search_query.to_lowercase();

    let display_files: Vec<(usize, &&crate::indexer::types::FileRecord)> =
        if has_search && !app.search_results.is_empty() {
            let matching_paths: std::collections::HashSet<&str> = app
                .search_results
                .iter()
                .filter_map(|&idx| app.files.get(idx))
                .map(|f| f.path.as_str())
                .collect();

            filtered
                .iter()
                .enumerate()
                .filter(|(_, file)| matching_paths.contains(file.path.as_str()))
                .collect()
        } else {
            filtered.iter().enumerate().collect()
        };

    let items: Vec<ListItem> = display_files
        .iter()
        .map(|&(i, file)| {
            let type_icon = match file.file_type.as_str() {
                "component" => "cmp",
                "module" => "mod",
                "migration" => "mig",
                "config" => "cfg",
                "style" => "sty",
                "docs" => "doc",
                "diagram" => "dia",
                "claude_md" => "ai ",
                "container" => "ctr",
                "script" => "scr",
                "env" => "env",
                _ => "   ",
            };

            let subdomain = if file.subdomain != "core" {
                format!(" [{}]", file.subdomain)
            } else {
                String::new()
            };

            let is_selected = i == app.selected_file;
            let path_str = format!("{type_icon} {}{subdomain}", file.path);

            let spans = if has_search && !query_lower.is_empty() {
                highlight_matches(&path_str, &query_lower, is_selected)
            } else {
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                vec![Span::styled(path_str, style)]
            };

            ListItem::new(Line::from(spans))
        })
        .collect();

    let domain_label = if app.show_all_files {
        "all".to_string()
    } else {
        app.domains
            .get(app.selected_domain)
            .cloned()
            .unwrap_or_else(|| "none".to_string())
    };

    let count_label = if has_search {
        format!("{}/{}", display_files.len(), filtered.len())
    } else {
        format!("{}", filtered.len())
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(format!(
                    " Files -- {domain_label} ({count_label}) [Tab:toggle] ",
                ))
                .title_style(Style::default().fg(Color::Yellow)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(list, area);
}

/// Classify a diagram_type string into an architectural level.
fn diagram_level(diagram_type: &str) -> &'static str {
    let dt = diagram_type.to_lowercase();
    if dt.contains("system") || dt == "flowchart" || dt == "graph" {
        "System"
    } else if dt.contains("container") || dt == "sequence" {
        "Container"
    } else if dt.contains("class") || dt == "erdiagram" || dt == "classdiagram" {
        "Class"
    } else {
        "Other"
    }
}

fn render_diagram_summary(frame: &mut Frame, area: Rect, app: &App) {
    // Filter diagrams to the currently selected domain
    let current_domain = app
        .domains
        .get(app.selected_domain)
        .cloned()
        .unwrap_or_default();

    let domain_diagrams: Vec<(usize, &crate::indexer::types::DiagramRecord)> = app
        .diagrams
        .iter()
        .enumerate()
        .filter(|(_, d)| app.show_all_files || d.domain == current_domain)
        .collect();

    // Group by level
    let levels = ["System", "Container", "Class", "Other"];
    let mut lines: Vec<Line<'static>> = Vec::new();

    for level in &levels {
        let in_level: Vec<&(usize, &crate::indexer::types::DiagramRecord)> = domain_diagrams
            .iter()
            .filter(|(_, d)| diagram_level(&d.diagram_type) == *level)
            .collect();

        if in_level.is_empty() {
            continue;
        }

        // Level header
        lines.push(Line::from(Span::styled(
            format!("-- {} --", level),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));

        for (global_idx, diag) in &in_level {
            let is_pinned = app.pinned_diagrams.contains(global_idx);
            let pinned_marker = if is_pinned { "*" } else { " " };

            let entry = format!(
                "[{}] {} ({} tokens)",
                pinned_marker, diag.id, diag.tokens_est
            );

            let style = if is_pinned {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            lines.push(Line::from(Span::styled(entry, style)));

            // Show truncated description when available
            if !diag.description.is_empty() {
                let desc = if diag.description.len() > 70 {
                    format!("    {}...", &diag.description[..67])
                } else {
                    format!("    {}", diag.description)
                };
                lines.push(Line::from(Span::styled(
                    desc,
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        // Blank separator between levels
        lines.push(Line::from(""));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No diagrams in this domain",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Split the right panel: diagram list | boundary+edge info | token gauge
    let has_boundaries = !app.boundaries.is_empty() || !app.edges.is_empty();
    let gauge_height: u16 = 3;
    let boundary_height: u16 = if has_boundaries { 8 } else { 0 };

    let mut constraints = vec![Constraint::Min(1)];
    if has_boundaries {
        constraints.push(Constraint::Length(boundary_height));
    }
    constraints.push(Constraint::Length(gauge_height));

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut chunk_idx = 0;

    // Diagram list
    let diag_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(
            " Diagrams ({}) ",
            domain_diagrams.len()
        ))
        .title_style(Style::default().fg(Color::Yellow));

    let paragraph = Paragraph::new(lines)
        .block(diag_block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, right_chunks[chunk_idx]);
    chunk_idx += 1;

    // Boundary + edge info (conditional)
    if has_boundaries {
        render_boundary_edges(frame, right_chunks[chunk_idx], app);
        chunk_idx += 1;
    }

    // Token gauge at the bottom
    let gauge = TokenGauge::new(app.pinned_tokens, app.token_budget).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Budget ")
            .title_style(Style::default().fg(Color::Yellow)),
    );

    frame.render_widget(gauge, right_chunks[chunk_idx]);
}

/// Render boundary types and cross-domain dependency edges.
fn render_boundary_edges(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Boundaries summary
    if !app.boundaries.is_empty() {
        for b in &app.boundaries {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}", b.domain),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!(" ({})", b.boundary_type.as_str()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
    }

    // Edges
    if !app.edges.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        for e in &app.edges {
            lines.push(Line::from(Span::styled(
                format!(
                    "{} -> {} ({})",
                    e.source_domain, e.target_domain, e.dep_name
                ),
                Style::default().fg(Color::White),
            )));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(
            " Boundaries ({}) Edges ({}) ",
            app.boundaries.len(),
            app.edges.len()
        ))
        .title_style(Style::default().fg(Color::Magenta));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

/// Build a span list that highlights case-insensitive matches of `query` within `text`.
///
/// Returns `Vec<Span<'static>>` -- all string data is owned by the spans themselves,
/// avoiding lifetime entanglement with the caller's locals.
fn highlight_matches(text: &str, query: &str, is_selected: bool) -> Vec<Span<'static>> {
    let base_style = if is_selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let match_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    let text_lower = text.to_lowercase();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut last_end = 0;

    for (start, _) in text_lower.match_indices(query) {
        let end = start + query.len();
        if start > last_end {
            spans.push(Span::styled(
                text[last_end..start].to_owned(),
                base_style,
            ));
        }
        spans.push(Span::styled(
            text[start..end].to_owned(),
            match_style,
        ));
        last_end = end;
    }

    if last_end < text.len() {
        spans.push(Span::styled(
            text[last_end..].to_owned(),
            base_style,
        ));
    }

    if spans.is_empty() {
        spans.push(Span::styled(text.to_owned(), base_style));
    }

    spans
}
