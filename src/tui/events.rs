//! Event handling for TUI
//!
//! This module handles keyboard and mouse events
//! and converts them into TUI actions.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};

/// Result of processing an event
#[derive(Debug, Clone)]
pub enum EventResult {
    /// User entered a command (submit for processing)
    Submit,

    /// User cancelled current action
    Cancel,

    /// User requested to interrupt agent (first Ctrl+C)
    InterruptAgent,

    /// User requested to exit (second Ctrl+C or Ctrl+D)
    Exit,

    /// User requested a role switch
    SwitchRole,

    /// User approved pending action
    ApproveAction,

    /// User rejected pending action
    RejectAction,

    /// User pressed a special key (no action)
    NoOp,

    /// User is typing (update input buffer)
    Input(char),

    /// User pressed backspace (delete character before cursor)
    Backspace,

    /// User pressed delete (delete character after cursor)
    Delete,

    /// User pressed left arrow (move cursor left)
    MoveLeft,

    /// User pressed right arrow (move cursor right)
    MoveRight,

    /// User pressed home (move cursor to start)
    MoveToStart,

    /// User pressed end (move cursor to end)
    MoveToEnd,

    /// User pressed up arrow (navigate history up)
    HistoryUp,

    /// User pressed down arrow (navigate history down)
    HistoryDown,

    /// User scrolled up
    ScrollUp,

    /// User scrolled down
    ScrollDown,

    /// User scrolled to top
    ScrollToTop,

    /// User scrolled to bottom
    ScrollToBottom,
}

/// Convert crossterm key event to event result
pub fn handle_key_event(event: KeyEvent) -> EventResult {
    // Ignore key releases and repeats
    if event.kind != KeyEventKind::Press {
        return EventResult::NoOp;
    }

    match event.code {
        // Enter - Submit command
        KeyCode::Enter => EventResult::Submit,

        // Escape - Cancel or switch roles
        KeyCode::Esc => EventResult::Cancel,

        // Ctrl+S - Submit (alternative to Enter)
        KeyCode::Char('s') if event.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL) => {
            EventResult::NoOp // Handled by input widget
        }

        // Ctrl+C - Interrupt agent (first press) or exit (second press within 2s)
        KeyCode::Char('c') if event.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL) => {
            EventResult::InterruptAgent
        }

        // Ctrl+D - Exit
        KeyCode::Char('d') if event.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL) => {
            EventResult::Exit
        }

        // Tab/Shift+Tab - Role switching (handled elsewhere)
        KeyCode::Tab | KeyCode::BackTab => EventResult::SwitchRole,

        // Ctrl+L - Clear screen
        KeyCode::Char('l') if event.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL) => {
            EventResult::NoOp // Handled by main loop
        }

        // Ctrl+U - Clear input
        KeyCode::Char('u') if event.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL) => {
            EventResult::Cancel // Clear input
        }

        // Arrow keys - Navigation
        KeyCode::Up => EventResult::HistoryUp,
        KeyCode::Down => EventResult::HistoryDown,
        KeyCode::Left => EventResult::MoveLeft,
        KeyCode::Right => EventResult::MoveRight,
        KeyCode::Home => EventResult::MoveToStart,
        KeyCode::End => EventResult::MoveToEnd,

        // Page keys - Scroll conversation
        KeyCode::PageUp => EventResult::ScrollUp,
        KeyCode::PageDown => EventResult::ScrollDown,

        // Backspace - Delete character before cursor
        KeyCode::Backspace => EventResult::Backspace,

        // Delete - Delete character after cursor
        KeyCode::Delete => EventResult::Delete,

        // Regular characters - Input (handled by input widget)
        KeyCode::Char(c) => EventResult::Input(c),

        // F-keys - Commands (handled elsewhere)
        KeyCode::F(1) | KeyCode::F(2) | KeyCode::F(3) | KeyCode::F(4) => EventResult::NoOp,
        KeyCode::F(5) | KeyCode::F(6) | KeyCode::F(7) | KeyCode::F(8) => EventResult::NoOp,
        KeyCode::F(9) | KeyCode::F(10) | KeyCode::F(11) | KeyCode::F(12) => EventResult::NoOp,

        // Other keys - No action
        _ => EventResult::NoOp,
    }
}

/// Convert crossterm mouse event to event result
pub fn handle_mouse_event(event: MouseEvent) -> EventResult {
    match event.kind {
        MouseEventKind::ScrollUp => EventResult::ScrollUp,
        MouseEventKind::ScrollDown => EventResult::ScrollDown,
        MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left) => {
            EventResult::NoOp // Handled by widgets
        }
        _ => EventResult::NoOp,
    }
}

/// Check if event should trigger a submit action
pub fn should_submit(event: &KeyEvent) -> bool {
    event.code == KeyCode::Enter && event.kind == KeyEventKind::Press
}

/// Check if event should trigger a cancel action
pub fn should_cancel(event: &KeyEvent) -> bool {
    event.code == KeyCode::Esc && event.kind == KeyEventKind::Press
}

/// Check if event should trigger an exit action
pub fn should_exit(event: &KeyEvent) -> bool {
    if event.kind != KeyEventKind::Press {
        return false;
    }

    match event.code {
        // Only Ctrl+D directly exits now (Ctrl+C is handled via InterruptAgent)
        KeyCode::Char('d') => {
            event.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL)
        }
        _ => false,
    }
}

/// Check if event is a role switch trigger
pub fn should_switch_role(event: &KeyEvent) -> bool {
    event.kind == KeyEventKind::Press
        && (event.code == KeyCode::Tab || event.code == KeyCode::BackTab)
}
