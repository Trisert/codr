//! Input widget for command entry
//!
//! Handles user input, history navigation,
//! and file/command picker overlays.

use crate::tui::theme::Theme;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Modifier,
    style::Style,
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

/// Input widget for entering commands
pub struct InputWidget {
    /// Current input text
    input: String,
    /// Cursor position
    cursor_position: usize,
    /// Command history
    history: Vec<String>,
    /// Current history index (None = at newest)
    history_index: Option<usize>,
    /// Placeholder text
    placeholder: String,
    /// Whether input is focused
    focused: bool,
    /// Theme for styling
    theme: Theme,
}

impl InputWidget {
    /// Create new input widget
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor_position: 0,
            history: Vec::new(),
            history_index: None,
            placeholder: "Enter a message...".to_string(),
            focused: true,
            theme: Theme::dark(),
        }
    }

    /// Set theme
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Set placeholder text
    pub fn placeholder(mut self, placeholder: String) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Set focus state
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Get current input
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Set input text
    pub fn set_input(&mut self, text: &str) {
        self.input = text.to_string();
        self.cursor_position = self.input.len();
    }

    /// Get cursor position
    pub fn cursor_position(&self) -> usize {
        self.cursor_position
    }

    /// Clear input
    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor_position = 0;
        self.history_index = None;
    }

    /// Submit input (add to history)
    pub fn submit(&mut self) -> Option<String> {
        if self.input.trim().is_empty() {
            return None;
        }

        let input = self.input.clone();
        self.history.push(input.clone());
        self.clear();
        Some(input)
    }

    /// Insert character at cursor
    pub fn insert(&mut self, ch: char) {
        self.input.insert(self.cursor_position, ch);
        self.cursor_position += 1;
        self.history_index = None;
    }

    /// Delete character at cursor (backspace)
    pub fn backspace(&mut self) {
        if self.cursor_position > 0 {
            self.input.remove(self.cursor_position - 1);
            self.cursor_position -= 1;
            self.history_index = None;
        }
    }

    /// Delete character after cursor (delete key)
    pub fn delete(&mut self) {
        if self.cursor_position < self.input.len() {
            self.input.remove(self.cursor_position);
            self.history_index = None;
        }
    }

    /// Move cursor left
    pub fn move_left(&mut self) {
        self.cursor_position = self.cursor_position.saturating_sub(1);
    }

    /// Move cursor right
    pub fn move_right(&mut self) {
        self.cursor_position = (self.cursor_position + 1).min(self.input.len());
    }

    /// Move cursor to start
    pub fn move_to_start(&mut self) {
        self.cursor_position = 0;
    }

    /// Move cursor to end
    pub fn move_to_end(&mut self) {
        self.cursor_position = self.input.len();
    }

    /// Navigate history up (older commands)
    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }

        if self.history_index.is_none() && !self.input.is_empty() {
            // Remember current unsaved input (not implemented yet)
        }

        let max_index = self.history.len().saturating_sub(1);
        let new_index = self
            .history_index
            .map(|i| i.saturating_add(1))
            .unwrap_or(0)
            .min(max_index);

        self.history_index = Some(new_index);
        if let Some(cmd) = self.history.get(self.history_index.unwrap()) {
            self.input = cmd.clone();
            self.cursor_position = self.input.len();
        }
    }

    /// Navigate history down (newer commands)
    pub fn history_down(&mut self) {
        if self.history_index.is_none() {
            return;
        }

        if let Some(index) = self.history_index {
            if index == 0 {
                self.input.clear();
                self.cursor_position = 0;
                self.history_index = None;
            } else {
                self.history_index = Some(index - 1);
                if let Some(cmd) = self.history.get(self.history_index.unwrap()) {
                    self.input = cmd.clone();
                    self.cursor_position = self.input.len();
                }
            }
        }
    }
}

impl Default for InputWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for InputWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Codex style: top separator + prompt symbol
        let border_style = Style::default().fg(self.theme.border);
        let prompt_style = Style::default()
            .fg(self.theme.primary)
            .add_modifier(Modifier::BOLD);
        let cursor_style = Style::default()
            .fg(self.theme.background)
            .bg(self.theme.primary);

        // Subtle top separator
        let top_border = "─".repeat(area.width as usize);
        buf.set_string(area.x, area.y, &top_border, border_style);

        // Prompt symbol ❯ in accent color
        let x = area.x + 1;
        let y = area.y + 1;
        buf.set_string(x, y, "❯", prompt_style);

        // Input text
        let input_x = x + 2;
        let input_style = Style::default().fg(self.theme.foreground);
        let max_width = (area.width as usize).saturating_sub(4);

        if self.input.is_empty() && !self.focused {
            // Placeholder
            let placeholder_style = Style::default()
                .fg(self.theme.dimmed)
                .add_modifier(Modifier::DIM);
            buf.set_string(input_x, y, &self.placeholder, placeholder_style);
        } else {
            // Input text
            let display_text = if self.input.len() > max_width {
                format!("{}…", &self.input[..max_width.saturating_sub(1)])
            } else {
                self.input.clone()
            };
            buf.set_string(input_x, y, &display_text, input_style);

            // Render cursor if focused
            if self.focused {
                let cursor_x = input_x + self.cursor_position.min(display_text.width()) as u16;
                if cursor_x < area.right() - 1 {
                    let cursor_pos = Position::new(cursor_x, y);
                    let cursor_cell = buf.cell_mut(cursor_pos);
                    if let Some(cell) = cursor_cell {
                        cell.set_char('▏');
                        cell.set_style(cursor_style);
                    }
                }
            }
        }
    }
}
