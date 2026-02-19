//! Keyboard input handling for the TUI.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::error::Result;
use super::app::{App, View};

/// Handle a key event. Returns `true` if the app should quit.
pub fn handle_key(key: KeyEvent, app: &mut App) -> Result<bool> {
    // Global keys (always active)
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Ok(true),
        (KeyModifiers::CONTROL, KeyCode::Char('q')) => return Ok(true),
        _ => {}
    }

    // If text input is focused, route to text handling
    if app.input_focused {
        return handle_text_input(key, app);
    }

    // Global navigation (when not in text input)
    match key.code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Char('1') => app.current_view = View::Index,
        KeyCode::Char('2') => app.current_view = View::Plan,
        KeyCode::Char('3') if !app.diagrams.is_empty() => {
            app.current_view = View::Diagram;
            app.render_current_diagram();
        }
        KeyCode::Char('4') if app.planning_context.is_some() => {
            app.current_view = View::Handoff;
        }
        KeyCode::Char('i') => {
            // Run indexer
            if let Err(e) = app.run_indexer() {
                app.status_message = format!("Indexer error: {e}");
            }
        }
        KeyCode::Char('?') => {
            app.status_message =
                "1:Index 2:Plan 3:Diagrams 4:Handoff i:Reindex q:Quit".to_string();
        }
        _ => {
            // Delegate to view-specific handler
            match app.current_view {
                View::Index => handle_index_keys(key, app),
                View::Plan => handle_plan_keys(key, app),
                View::Diagram => handle_diagram_keys(key, app),
                View::Handoff => handle_handoff_keys(key, app)?,
            }
        }
    }

    Ok(false)
}

fn handle_text_input(key: KeyEvent, app: &mut App) -> Result<bool> {
    match key.code {
        KeyCode::Esc => {
            app.input_focused = false;
        }
        KeyCode::Enter => {
            app.input_focused = false;
            // Generate plan when Enter is pressed
            if let Err(e) = app.generate_plan() {
                app.status_message = format!("Plan error: {e}");
            }
        }
        KeyCode::Backspace => {
            app.change_description.pop();
        }
        KeyCode::Char(c) => {
            app.change_description.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_index_keys(key: KeyEvent, app: &mut App) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            app.selected_file = app.selected_file.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let max = app.filtered_files().len().saturating_sub(1);
            app.selected_file = (app.selected_file + 1).min(max);
        }
        KeyCode::Left | KeyCode::Char('h') => {
            app.selected_domain = app.selected_domain.saturating_sub(1);
            app.selected_file = 0;
        }
        KeyCode::Right | KeyCode::Char('l') => {
            let max = app.domains.len().saturating_sub(1);
            app.selected_domain = (app.selected_domain + 1).min(max);
            app.selected_file = 0;
        }
        KeyCode::Tab => {
            app.show_all_files = !app.show_all_files;
            app.selected_file = 0;
        }
        _ => {}
    }
}

fn handle_plan_keys(key: KeyEvent, app: &mut App) {
    match key.code {
        KeyCode::Char('e') | KeyCode::Enter => {
            // Focus the text input
            app.input_focused = true;
        }
        KeyCode::Char('g') => {
            // Generate plan
            if let Err(e) = app.generate_plan() {
                app.status_message = format!("Plan error: {e}");
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            // Navigate scopes
        }
        KeyCode::Down | KeyCode::Char('j') => {
            // Navigate scopes
        }
        KeyCode::Char(' ') => {
            // Toggle scope selection
        }
        _ => {}
    }
}

fn handle_diagram_keys(key: KeyEvent, app: &mut App) {
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            if app.selected_diagram > 0 {
                app.selected_diagram -= 1;
                app.diagram_scroll = 0;
                app.render_current_diagram();
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if app.selected_diagram + 1 < app.diagrams.len() {
                app.selected_diagram += 1;
                app.diagram_scroll = 0;
                app.render_current_diagram();
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.diagram_scroll = app.diagram_scroll.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.diagram_scroll += 1;
        }
        KeyCode::Home => {
            app.diagram_scroll = 0;
        }
        _ => {}
    }
}

fn handle_handoff_keys(key: KeyEvent, app: &mut App) -> Result<()> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            app.handoff_scroll = app.handoff_scroll.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.handoff_scroll += 1;
        }
        KeyCode::Char('x') => {
            // Export to file
            if let Err(e) = app.export_plan() {
                app.status_message = format!("Export error: {e}");
            } else {
                app.status_message = "Exported to .claude/planner-context.md".to_string();
            }
        }
        KeyCode::Home => {
            app.handoff_scroll = 0;
        }
        _ => {}
    }
    Ok(())
}
