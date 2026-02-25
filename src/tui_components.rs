// Utility components and helpers for codr TUI
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthStr, UnicodeWidthChar};

// ============================================================
// Utility Functions
// ============================================================

/// Calculate visible width accounting for ANSI codes
pub fn visible_width(text: &str) -> usize {
    let stripped = strip_ansi(text);
    stripped.width()
}

/// Strip ANSI escape codes from text
pub fn strip_ansi(text: &str) -> String {
    let mut result = String::new();
    let mut in_escape = false;

    for c in text.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Truncate text to fit width with ellipsis
pub fn truncate_to_width(text: &str, width: usize) -> String {
    let mut result = String::new();
    let mut current_width = 0;

    for c in text.chars() {
        let char_width = c.width().unwrap_or(0);
        if current_width + char_width > width {
            if current_width + 3 <= width {
                result.push_str("...");
            }
            break;
        }
        result.push(c);
        current_width += char_width;
    }
    result
}

// ============================================================
// Styled Text Builders
// ============================================================

pub struct StyledText {
    spans: Vec<Span<'static>>,
}

impl StyledText {
    pub fn new() -> Self {
        Self {
            spans: Vec::new(),
        }
    }

    pub fn plain(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::raw(text.into()));
        self
    }

    pub fn bold(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(text.into(), Style::default().add_modifier(Modifier::BOLD)));
        self
    }

    pub fn italic(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(text.into(), Style::default().add_modifier(Modifier::ITALIC)));
        self
    }

    pub fn cyan(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(text.into(), Style::default().fg(Color::Cyan)));
        self
    }

    pub fn green(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(text.into(), Style::default().fg(Color::Green)));
        self
    }

    pub fn yellow(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(text.into(), Style::default().fg(Color::Yellow)));
        self
    }

    pub fn red(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(text.into(), Style::default().fg(Color::Red)));
        self
    }

    pub fn blue(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(text.into(), Style::default().fg(Color::Blue)));
        self
    }

    pub fn gray(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(text.into(), Style::default().fg(Color::DarkGray)));
        self
    }

    pub fn dim(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(text.into(), Style::default().add_modifier(Modifier::DIM)));
        self
    }

    pub fn into_line(self) -> Line<'static> {
        Line::from(self.spans)
    }
}

impl Default for StyledText {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================
// Role-based Message Styling
// ============================================================

pub fn style_role(role: &str) -> Style {
    match role {
        "user" => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        "assistant" => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        "action" => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        "output" => Style::default().fg(Color::Blue),
        "error" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        "system" => Style::default().fg(Color::Gray),
        _ => Style::default(),
    }
}

pub fn get_role_label(role: &str) -> &str {
    match role {
        "user" => "👤 You",
        "assistant" => "🤖 codr",
        "action" => "🔧 Action",
        "output" => "📤 Output",
        "error" => "⚠️ Error",
        _ => role,
    }
}

// ============================================================
// Key Bindings Display
// ============================================================

pub struct Keybinding {
    pub key: &'static str,
    pub description: &'static str,
}

pub const DEFAULT_KEYBINDINGS: &[Keybinding] = &[
    Keybinding { key: "Ctrl+S", description: "Send message" },
    Keybinding { key: "Ctrl+Q", description: "Quit" },
    Keybinding { key: "Ctrl+C", description: "Quit" },
    Keybinding { key: "Arrows", description: "Move cursor" },
    Keybinding { key: "Enter", description: "Insert newline" },
];

pub fn render_keybindings() -> Vec<Line<'static>> {
    vec![StyledText::new()
        .gray("Keybindings: ")
        .dim("Ctrl+S: Send | Ctrl+Q: Quit")
        .into_line()]
}

// ============================================================
// Markdown rendering (simplified)
// ============================================================

pub fn render_markdown(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();

    for line in text.lines() {
        let processed = process_inline_markdown(line);
        lines.extend(wrap_text(&processed, width));
    }

    lines
}

fn process_inline_markdown(line: &str) -> String {
    let mut result = line.to_string();

    // Code blocks: `text`
    while let Some(start) = result.find('`') {
        if let Some(end) = result[start + 1..].find('`') {
            result = format!("{}{}{}{}{}",
                &result[..start],
                "\x1b[35m", // Magenta
                &result[start + 1..start + 1 + end],
                "\x1b[0m",
                &result[start + 1 + end + 1..]
            );
        } else {
            break;
        }
    }

    result
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in text.split(' ') {
        let word_width = word.chars().map(|c| c.to_string().width()).sum::<usize>();

        if current_width + word_width > width && !current_line.is_empty() {
            lines.push(current_line);
            current_line = String::new();
            current_width = 0;
        }

        if !current_line.is_empty() {
            current_line.push(' ');
            current_width += 1;
        }

        current_line.push_str(word);
        current_width += word_width;
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    lines
}

// ============================================================
// Re-export for convenience
// ============================================================

pub use crate::tui::{ChatMessage, App, run_tui};
