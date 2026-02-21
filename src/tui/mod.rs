//! Interactive TUI for llmermaid.
//!
//! Provides a multi-panel interface for:
//! - Browsing the file index by domain
//! - Selecting scopes for a change
//! - Viewing mermaid diagrams (ASCII rendering)
//! - Writing change descriptions interactively
//! - Reviewing and exporting the planning context
//!
//! Architecture: The Elm Architecture (TEA)
//! - `message.rs` defines the `Message` enum (all possible events)
//! - `input.rs` maps `KeyEvent` -> `Option<Message>` (pure, no side effects)
//! - `app.rs` owns `App::update(msg)` (sole point of state mutation)
//! - `views/` reads `&App` immutably for rendering

pub mod app;
pub mod input;
pub mod message;
pub mod views;
pub mod widgets;

use std::io::{self, Write as _};
use std::path::Path;
use std::time::Duration;

use crossterm::event::{self, Event};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use tracing::info;

use crate::error::{Error, Result};
use app::App;
use message::Message;

/// Run the interactive TUI.
pub fn run(root: &Path) -> Result<()> {
    info!(root = %root.display(), "Starting TUI");

    // Initialize terminal
    enable_raw_mode().map_err(|e| Error::Terminal(e.to_string()))?;
    io::stdout()
        .execute(EnterAlternateScreen)
        .map_err(|e| Error::Terminal(e.to_string()))?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal =
        Terminal::new(backend).map_err(|e| Error::Terminal(e.to_string()))?;

    // Create app state
    let mut app = App::new(root)?;

    // Main loop
    let result = main_loop(&mut terminal, &mut app);

    // Restore terminal (always, even on error)
    let _ = disable_raw_mode();
    let _ = io::stdout().execute(LeaveAlternateScreen);

    result
}

/// TEA main loop: render -> poll events -> map to message -> update state.
fn main_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        // Render
        terminal
            .draw(|frame| views::render(frame, app))
            .map_err(|e| Error::Terminal(e.to_string()))?;

        // Poll events (100ms timeout for responsive UI + spinner animation)
        if event::poll(Duration::from_millis(100))
            .map_err(|e| Error::Terminal(e.to_string()))?
        {
            if let Event::Key(key) = event::read().map_err(|e| Error::Terminal(e.to_string()))? {
                if let Some(msg) = input::key_to_message(key, app) {
                    if app.update(msg)? {
                        break;
                    }
                }
            }
        }

        // Handle pending editor request (side-effect from OpenExternalEditor).
        // We must own the terminal to leave/re-enter alternate screen.
        if let Some(content) = app.editor_request.take() {
            handle_editor_request(terminal, app, &content)?;
        }

        // Poll for messages from background threads (e.g. indexer).
        if let Some(rx) = &app.async_rx {
            if let Ok(msg) = rx.try_recv() {
                // Drop the receiver before calling update, since update
                // might set a new one (unlikely but clean).
                app.async_rx = None;
                if app.update(msg)? {
                    break;
                }
            }
        }

        // Animate spinner if an async operation is in progress
        if app.is_busy {
            app.spinner_frame = (app.spinner_frame + 1) % 10;
        }
    }

    Ok(())
}

/// Pause the TUI, spawn `$EDITOR` with the content in a temp file,
/// read back the result, and resume the TUI.
fn handle_editor_request(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    content: &str,
) -> Result<()> {
    use std::process::Command;
    use tempfile::NamedTempFile;

    // Resolve editor: $VISUAL > $EDITOR > settings.general.editor > vi
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .ok()
        .or_else(|| app.settings.general.editor.clone())
        .unwrap_or_else(|| "vi".to_string());

    // Write current content to a temp file (suffix helps editors with syntax)
    let mut tmp = NamedTempFile::with_suffix(".md")
        .map_err(|e| Error::Terminal(format!("Failed to create temp file: {e}")))?;
    tmp.write_all(content.as_bytes())
        .map_err(|e| Error::Terminal(format!("Failed to write temp file: {e}")))?;
    tmp.flush()
        .map_err(|e| Error::Terminal(format!("Failed to flush temp file: {e}")))?;
    let tmp_path = tmp.path().to_path_buf();

    // Leave alternate screen and disable raw mode so the editor gets
    // a normal terminal. Order matters: leave alt screen first so the
    // editor doesn't paint over our buffer.
    io::stdout()
        .execute(LeaveAlternateScreen)
        .map_err(|e| Error::Terminal(e.to_string()))?;
    disable_raw_mode().map_err(|e| Error::Terminal(e.to_string()))?;

    // Spawn the editor and wait for it to exit.
    let status = Command::new(&editor)
        .arg(&tmp_path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();

    // Re-enter raw mode and alternate screen unconditionally so we
    // always restore the TUI, even if the editor failed.
    enable_raw_mode().map_err(|e| Error::Terminal(e.to_string()))?;
    io::stdout()
        .execute(EnterAlternateScreen)
        .map_err(|e| Error::Terminal(e.to_string()))?;

    // Force a full redraw after returning from the editor. The terminal
    // buffer is stale because another process owned the screen.
    terminal
        .clear()
        .map_err(|e| Error::Terminal(e.to_string()))?;

    match status {
        Ok(s) if s.success() => {
            // Read back the (possibly edited) content
            let new_content = std::fs::read_to_string(&tmp_path)
                .map_err(|e| Error::Terminal(format!("Failed to read temp file: {e}")))?;
            app.update(Message::EditorComplete(new_content))?;
        }
        Ok(s) => {
            let code = s.code().unwrap_or(-1);
            app.update(Message::SetStatus(
                format!("Editor exited with code {code}"),
                message::StatusLevel::Warning,
            ))?;
        }
        Err(e) => {
            app.update(Message::SetStatus(
                format!("Failed to launch editor '{editor}': {e}"),
                message::StatusLevel::Error,
            ))?;
        }
    }

    Ok(())
}
