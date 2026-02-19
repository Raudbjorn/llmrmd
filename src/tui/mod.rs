//! Interactive TUI for llmermaid.
//!
//! Provides a multi-panel interface for:
//! - Browsing the file index by domain
//! - Selecting scopes for a change
//! - Viewing mermaid diagrams (ASCII rendering)
//! - Writing change descriptions interactively
//! - Reviewing and exporting the planning context

pub mod app;
pub mod input;
pub mod views;
pub mod widgets;

use std::io;
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

fn main_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        // Draw
        terminal
            .draw(|frame| views::render(frame, app))
            .map_err(|e| Error::Terminal(e.to_string()))?;

        // Handle events (100ms poll timeout for responsive UI)
        if event::poll(Duration::from_millis(100))
            .map_err(|e| Error::Terminal(e.to_string()))?
        {
            if let Event::Key(key) = event::read().map_err(|e| Error::Terminal(e.to_string()))? {
                if input::handle_key(key, app)? {
                    // App requested exit
                    break;
                }
            }
        }
    }

    Ok(())
}
