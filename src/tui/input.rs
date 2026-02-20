//! Keyboard input handling for the TUI.
//!
//! This module is a **pure mapper**: it converts `KeyEvent` values into
//! `Message` values with zero side effects. All state mutation happens
//! in `App::update()`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, PlanPhase, View};
use super::message::Message;

/// Map a key event to a message. Returns `None` if the key is unbound.
///
/// This function reads `&App` only to determine the current mode
/// (which view, whether input is focused, etc.) and never mutates state.
pub fn key_to_message(key: KeyEvent, app: &App) -> Option<Message> {
    // Global interrupt keys (always active, highest priority)
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Message::Quit),
        (KeyModifiers::CONTROL, KeyCode::Char('q')) => return Some(Message::Quit),
        (KeyModifiers::CONTROL, KeyCode::Char('e')) => return Some(Message::OpenExternalEditor),
        _ => {}
    }

    // Help overlay: any key dismisses it
    if app.show_help {
        return Some(Message::HideHelp);
    }

    // Fuzzy finder overlay: route all keys to fuzzy handling
    if app.fuzzy_open {
        return match key.code {
            KeyCode::Esc => Some(Message::FuzzyClose),
            KeyCode::Enter => Some(Message::FuzzyConfirm),
            KeyCode::Backspace => Some(Message::FuzzyBackspace),
            KeyCode::Up => Some(Message::FuzzySelectUp),
            KeyCode::Down => Some(Message::FuzzySelectDown),
            KeyCode::Char(c) => Some(Message::FuzzyInput(c)),
            _ => None,
        };
    }

    // Diagram edit mode: route to diagram editing
    if app.diagram_edit_mode {
        return match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('s')) => {
                Some(Message::DiagramEditSave)
            }
            (_, KeyCode::Esc) => Some(Message::DiagramEditCancel),
            (_, KeyCode::Enter) => Some(Message::DiagramEditNewline),
            (_, KeyCode::Backspace) => Some(Message::DiagramEditBackspace),
            (_, KeyCode::Char(c)) => Some(Message::DiagramEditChar(c)),
            _ => None,
        };
    }

    // Search mode: route all keys to search handling
    if app.search_active {
        return match key.code {
            KeyCode::Esc => Some(Message::SearchClear),
            KeyCode::Enter => Some(Message::SearchSubmit),
            KeyCode::Backspace => Some(Message::SearchBackspace),
            KeyCode::Char(c) => Some(Message::SearchInput(c)),
            _ => None,
        };
    }

    // Text input mode: route to change description editing
    if app.input_focused {
        return match key.code {
            KeyCode::Esc => Some(Message::UnfocusInput),
            KeyCode::Enter => Some(Message::InputSubmit),
            KeyCode::Backspace => Some(Message::InputBackspace),
            KeyCode::Char(c) => Some(Message::InputChar(c)),
            _ => None,
        };
    }

    // Global navigation (when not in any text input mode)
    match key.code {
        KeyCode::Char('q') => return Some(Message::Quit),
        KeyCode::Char('1') => return Some(Message::SwitchView(View::Index)),
        KeyCode::Char('2') => return Some(Message::SwitchView(View::Plan)),
        KeyCode::Char('3') => return Some(Message::SwitchView(View::Diagram)),
        KeyCode::Char('4') => return Some(Message::SwitchView(View::Handoff)),
        KeyCode::Char('i') => return Some(Message::RunIndexer),
        KeyCode::Char('?') => return Some(Message::ShowHelp),
        KeyCode::Char('/') => return Some(Message::SearchStart),
        KeyCode::Char('+') => return Some(Message::IncreaseBudget),
        KeyCode::Char('-') => return Some(Message::DecreaseBudget),
        _ => {}
    }

    // Ctrl combinations for non-input modes
    if matches!(
        (key.modifiers, key.code),
        (KeyModifiers::CONTROL, KeyCode::Char('f'))
    ) {
        return Some(Message::FuzzyOpen);
    }

    // View-specific key mapping
    match app.current_view {
        View::Index => index_key_to_message(key),
        View::Plan => plan_key_to_message(key, app),
        View::Diagram => diagram_key_to_message(key, app),
        View::Handoff => handoff_key_to_message(key),
    }
}

/// Map index view keys to messages.
fn index_key_to_message(key: KeyEvent) -> Option<Message> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(Message::MoveFileUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Message::MoveFileDown),
        KeyCode::Left | KeyCode::Char('h') => Some(Message::MoveDomainUp),
        KeyCode::Right | KeyCode::Char('l') => Some(Message::MoveDomainDown),
        KeyCode::Tab => Some(Message::ToggleShowAllDomains),
        _ => None,
    }
}

/// Map plan view keys to messages.
///
/// Takes `&App` to resolve the current plan phase and selected scope.
/// In `Verify` phase, Enter/r act as approve/reject gates.
fn plan_key_to_message(key: KeyEvent, app: &App) -> Option<Message> {
    match app.plan_phase {
        PlanPhase::Verify => match key.code {
            KeyCode::Enter => Some(Message::ApprovePlan),
            KeyCode::Char('r') => Some(Message::RejectPlan),
            KeyCode::Char('e') => Some(Message::EditDiagramInPlace),
            KeyCode::Up | KeyCode::Char('k') => Some(Message::MoveScopeUp),
            KeyCode::Down | KeyCode::Char('j') => Some(Message::MoveScopeDown),
            _ => None,
        },
        _ => match key.code {
            KeyCode::Char('e') | KeyCode::Enter => Some(Message::FocusInput),
            KeyCode::Char('g') => Some(Message::GeneratePlan),
            KeyCode::Up | KeyCode::Char('k') => Some(Message::MoveScopeUp),
            KeyCode::Down | KeyCode::Char('j') => Some(Message::MoveScopeDown),
            KeyCode::Char(' ') => {
                Some(Message::ToggleScope(app.selected_scope))
            }
            _ => None,
        },
    }
}

/// Map diagram view keys to messages.
fn diagram_key_to_message(key: KeyEvent, app: &App) -> Option<Message> {
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => Some(Message::PrevDiagram),
        KeyCode::Right | KeyCode::Char('l') => Some(Message::NextDiagram),
        KeyCode::Up | KeyCode::Char('k') => Some(Message::ScrollDiagramUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Message::ScrollDiagramDown),
        KeyCode::Home => Some(Message::ScrollDiagramHome),
        KeyCode::Char(' ') => {
            Some(Message::TogglePinDiagram(app.selected_diagram))
        }
        KeyCode::Char('r') => Some(Message::AutoHealDiagram),
        KeyCode::Char('e') => Some(Message::EditDiagramInPlace),
        _ => None,
    }
}

/// Map handoff view keys to messages.
fn handoff_key_to_message(key: KeyEvent) -> Option<Message> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(Message::ScrollHandoffUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Message::ScrollHandoffDown),
        KeyCode::Home => Some(Message::ScrollHandoffHome),
        KeyCode::Char('x') => Some(Message::ExportPlan),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Create a minimal App for testing key mappings.
    fn test_app() -> App {
        // Use a temp path that won't have index artifacts
        App {
            root: Path::new("/tmp/test").to_path_buf(),
            current_view: View::Index,
            should_quit: false,
            files: Vec::new(),
            diagrams: Vec::new(),
            manifest: Vec::new(),
            domains: Vec::new(),
            claude_md_paths: Vec::new(),
            selected_domain: 0,
            selected_file: 0,
            show_all_files: false,
            change_description: String::new(),
            input_focused: false,
            selected_scopes: std::collections::HashSet::new(),
            inferred_scopes: Vec::new(),
            token_budget: 4000,
            selected_scope: 0,
            selected_diagram: 0,
            rendered_diagram: String::new(),
            diagram_scroll: 0,
            planning_context: None,
            handoff_scroll: 0,
            status_message: String::new(),
            status_level: super::super::message::StatusLevel::Info,
            show_help: false,
            search_active: false,
            search_query: String::new(),
            search_results: Vec::new(),
            is_busy: false,
            spinner_frame: 0,
            settings: crate::settings::Settings::default(),
            editor_request: None,
            async_rx: None,
            pinned_diagrams: std::collections::HashSet::new(),
            pinned_tokens: 0,
            plan_phase: PlanPhase::Describe,
            diagram_edit_mode: false,
            diagram_edit_buffer: String::new(),
            lint_issues: Vec::new(),
            agent_steps: Vec::new(),
            active_agent_step: 0,
            fuzzy_open: false,
            fuzzy_query: String::new(),
            fuzzy_results: Vec::new(),
            fuzzy_selected: 0,
        }
    }

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn make_ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_c_quits() {
        let app = test_app();
        let msg = key_to_message(make_ctrl_key('c'), &app);
        assert!(matches!(msg, Some(Message::Quit)));
    }

    #[test]
    fn ctrl_q_quits() {
        let app = test_app();
        let msg = key_to_message(make_ctrl_key('q'), &app);
        assert!(matches!(msg, Some(Message::Quit)));
    }

    #[test]
    fn q_quits_when_not_in_input() {
        let app = test_app();
        let msg = key_to_message(make_key(KeyCode::Char('q')), &app);
        assert!(matches!(msg, Some(Message::Quit)));
    }

    #[test]
    fn number_keys_switch_view() {
        let app = test_app();
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('1')), &app),
            Some(Message::SwitchView(View::Index))
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('2')), &app),
            Some(Message::SwitchView(View::Plan))
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('3')), &app),
            Some(Message::SwitchView(View::Diagram))
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('4')), &app),
            Some(Message::SwitchView(View::Handoff))
        ));
    }

    #[test]
    fn index_view_navigation() {
        let app = test_app();
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('j')), &app),
            Some(Message::MoveFileDown)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('k')), &app),
            Some(Message::MoveFileUp)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('h')), &app),
            Some(Message::MoveDomainUp)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('l')), &app),
            Some(Message::MoveDomainDown)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Tab), &app),
            Some(Message::ToggleShowAllDomains)
        ));
    }

    #[test]
    fn input_focused_routes_to_text_input() {
        let mut app = test_app();
        app.input_focused = true;

        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('a')), &app),
            Some(Message::InputChar('a'))
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Backspace), &app),
            Some(Message::InputBackspace)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Enter), &app),
            Some(Message::InputSubmit)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Esc), &app),
            Some(Message::UnfocusInput)
        ));
    }

    #[test]
    fn search_mode_routes_to_search() {
        let mut app = test_app();
        app.search_active = true;

        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('x')), &app),
            Some(Message::SearchInput('x'))
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Backspace), &app),
            Some(Message::SearchBackspace)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Enter), &app),
            Some(Message::SearchSubmit)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Esc), &app),
            Some(Message::SearchClear)
        ));
    }

    #[test]
    fn help_overlay_dismissed_by_any_key() {
        let mut app = test_app();
        app.show_help = true;

        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('x')), &app),
            Some(Message::HideHelp)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Enter), &app),
            Some(Message::HideHelp)
        ));
    }

    #[test]
    fn diagram_view_navigation() {
        let mut app = test_app();
        app.current_view = View::Diagram;

        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('h')), &app),
            Some(Message::PrevDiagram)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('l')), &app),
            Some(Message::NextDiagram)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('k')), &app),
            Some(Message::ScrollDiagramUp)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('j')), &app),
            Some(Message::ScrollDiagramDown)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Home), &app),
            Some(Message::ScrollDiagramHome)
        ));
    }

    #[test]
    fn handoff_view_navigation() {
        let mut app = test_app();
        app.current_view = View::Handoff;

        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('k')), &app),
            Some(Message::ScrollHandoffUp)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('j')), &app),
            Some(Message::ScrollHandoffDown)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Home), &app),
            Some(Message::ScrollHandoffHome)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('x')), &app),
            Some(Message::ExportPlan)
        ));
    }

    #[test]
    fn plan_view_keys() {
        let mut app = test_app();
        app.current_view = View::Plan;

        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('e')), &app),
            Some(Message::FocusInput)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('g')), &app),
            Some(Message::GeneratePlan)
        ));
        // With selected_scope == 0 (default), Space toggles scope 0
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char(' ')), &app),
            Some(Message::ToggleScope(0))
        ));
    }

    #[test]
    fn plan_view_space_uses_selected_scope() {
        let mut app = test_app();
        app.current_view = View::Plan;
        app.selected_scope = 3;

        let msg = key_to_message(make_key(KeyCode::Char(' ')), &app);
        assert!(matches!(msg, Some(Message::ToggleScope(3))));
    }

    #[test]
    fn budget_keys() {
        let app = test_app();
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('+')), &app),
            Some(Message::IncreaseBudget)
        ));
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('-')), &app),
            Some(Message::DecreaseBudget)
        ));
    }

    #[test]
    fn search_start() {
        let app = test_app();
        assert!(matches!(
            key_to_message(make_key(KeyCode::Char('/')), &app),
            Some(Message::SearchStart)
        ));
    }

    #[test]
    fn ctrl_c_overrides_input_mode() {
        let mut app = test_app();
        app.input_focused = true;
        // Ctrl+C should quit even in input mode
        let msg = key_to_message(make_ctrl_key('c'), &app);
        assert!(matches!(msg, Some(Message::Quit)));
    }

    #[test]
    fn unbound_key_returns_none() {
        let app = test_app();
        let msg = key_to_message(make_key(KeyCode::F(12)), &app);
        assert!(msg.is_none());
    }

    #[test]
    fn ctrl_e_opens_external_editor() {
        let app = test_app();
        let msg = key_to_message(make_ctrl_key('e'), &app);
        assert!(matches!(msg, Some(Message::OpenExternalEditor)));
    }
}
