//! Index view: browse domains and files.

use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    // Split: domains on left, files on right
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(area);

    render_domains(frame, chunks[0], app);
    render_files(frame, chunks[1], app);
}

fn render_domains(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .domains
        .iter()
        .enumerate()
        .map(|(i, domain)| {
            let file_count = app.files.iter().filter(|f| &f.domain == domain).count();
            let diag_count = app.diagrams.iter().filter(|d| &d.domain == domain).count();

            let mut line = format!("{domain}  ({file_count}");
            if diag_count > 0 {
                line.push_str(&format!(", {diag_count}📊"));
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

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, file)| {
            let type_icon = match file.file_type.as_str() {
                "component" => "🧩",
                "module" => "📦",
                "migration" => "🗄️",
                "config" => "⚙️",
                "style" => "🎨",
                "docs" => "📝",
                "diagram" => "📊",
                "claude_md" => "🤖",
                "container" => "🐳",
                "script" => "📜",
                "env" => "🔒",
                _ => "📄",
            };

            let style = if i == app.selected_file {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let subdomain = if file.subdomain != "core" {
                format!(" [{}]", file.subdomain)
            } else {
                String::new()
            };

            ListItem::new(format!("{type_icon} {}{subdomain}", file.path)).style(style)
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

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(format!(
                    " Files — {domain_label} ({}) [Tab:toggle] ",
                    filtered.len()
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
