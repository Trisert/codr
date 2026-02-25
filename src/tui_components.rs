use once_cell::sync::Lazy;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

#[derive(Clone, Debug, PartialEq)]
pub enum ApprovalState {
    None,
    Pending,
    Approved,
    Rejected,
}

#[derive(Clone, Debug)]
pub struct PendingAction {
    pub action_type: String,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

impl ChatMessage {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: chrono_timestamp(),
        }
    }

    pub fn user(content: &str) -> Self {
        Self::new("user", content)
    }

    pub fn assistant(content: &str) -> Self {
        Self::new("assistant", content)
    }

    pub fn system(content: &str) -> Self {
        Self::new("system", content)
    }

    pub fn action(content: &str) -> Self {
        Self::new("action", content)
    }

    pub fn output(content: &str) -> Self {
        Self::new("output", content)
    }

    pub fn error(content: &str) -> Self {
        Self::new("error", content)
    }
}

fn chrono_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mins = (secs / 60) % 60;
    let hours = (secs / 3600) % 24;
    format!("{:02}:{:02}", hours, mins)
}

pub struct StatusLine {
    pub model: String,
    pub tokens: u32,
    pub cost: f64,
    pub cwd: String,
}

impl StatusLine {
    pub fn new(model: &str) -> Self {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        Self {
            model: model.to_string(),
            tokens: 0,
            cost: 0.0,
            cwd,
        }
    }

    pub fn update(&mut self, tokens: u32, cost: f64) {
        self.tokens = tokens;
        self.cost = cost;
    }
}

pub struct MarkdownRenderer {
    syntax_set: SyntaxSet,
    theme: syntect::highlighting::Theme,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme = THEME_SET.themes["base16-ocean.dark"].clone();

        Self { syntax_set, theme }
    }

    pub fn render(&self, text: &str) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        for line in text.lines() {
            let processed = self.process_markdown_line(line);
            lines.push(processed);
        }

        lines
    }

    fn process_markdown_line(&self, line: &str) -> Line<'static> {
        if line.starts_with("# ") {
            return Line::from(vec![Span::styled(
                line[2..].to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED),
            )]);
        }
        if line.starts_with("## ") {
            return Line::from(vec![Span::styled(
                line[3..].to_string(),
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            )]);
        }
        if line.starts_with("### ") {
            return Line::from(vec![Span::styled(
                line[4..].to_string(),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )]);
        }

        if line.starts_with("- ") || line.starts_with("* ") {
            return Line::from(vec![
                Span::styled("  - ".to_string(), Style::default().fg(Color::Yellow)),
                Span::raw(line[2..].to_string()),
            ]);
        }

        if line.contains('`') {
            return self.render_inline_code(line);
        }

        if line.contains("**") {
            return self.render_bold(line);
        }

        Line::from(Span::raw(line.to_string()))
    }

    fn render_inline_code(&self, line: &str) -> Line<'static> {
        let mut spans = Vec::new();
        let mut remaining = line;

        while let Some(start) = remaining.find('`') {
            if start > 0 {
                spans.push(Span::raw(remaining[..start].to_string()));
            }

            if let Some(end) = remaining[start + 1..].find('`') {
                let code = &remaining[start + 1..start + 1 + end];
                spans.push(Span::styled(
                    code.to_string(),
                    Style::default()
                        .fg(Color::Rgb(255, 200, 120))
                        .add_modifier(Modifier::DIM),
                ));
                remaining = &remaining[start + end + 2..];
            } else {
                spans.push(Span::raw(remaining[start..].to_string()));
                break;
            }
        }

        if !remaining.is_empty() {
            spans.push(Span::raw(remaining.to_string()));
        }

        Line::from(spans)
    }

    fn render_bold(&self, line: &str) -> Line<'static> {
        let mut spans = Vec::new();
        let mut remaining = line;

        while let Some(start) = remaining.find("**") {
            if start > 0 {
                spans.push(Span::raw(remaining[..start].to_string()));
            }

            remaining = &remaining[start + 2..];

            if let Some(end) = remaining.find("**") {
                spans.push(Span::styled(
                    remaining[..end].to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                remaining = &remaining[end + 2..];
            } else {
                spans.push(Span::raw("**".to_string()));
                spans.push(Span::raw(remaining.to_string()));
                break;
            }
        }

        if !remaining.is_empty() {
            spans.push(Span::raw(remaining.to_string()));
        }

        Line::from(spans)
    }

    pub fn highlight_code(&self, code: &str, language: &str) -> Vec<Line<'static>> {
        let syntax = self
            .syntax_set
            .find_syntax_by_token(language)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut lines = Vec::new();

        for line in code.lines() {
            let ranges: Vec<(syntect::highlighting::Style, &str)> = highlighter
                .highlight_line(line, &self.syntax_set)
                .unwrap_or_else(|_| vec![(syntect::highlighting::Style::default(), line)]);

            let spans: Vec<Span> = ranges
                .iter()
                .map(|(style, text)| {
                    let fg = style.foreground;
                    let color = Color::Rgb(fg.r, fg.g, fg.b);
                    let mut ratatui_style = Style::default().fg(color);

                    if style
                        .font_style
                        .contains(syntect::highlighting::FontStyle::BOLD)
                    {
                        ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
                    }
                    if style
                        .font_style
                        .contains(syntect::highlighting::FontStyle::ITALIC)
                    {
                        ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
                    }
                    if style
                        .font_style
                        .contains(syntect::highlighting::FontStyle::UNDERLINE)
                    {
                        ratatui_style = ratatui_style.add_modifier(Modifier::UNDERLINED);
                    }

                    Span::styled(text.to_string(), ratatui_style)
                })
                .collect();

            lines.push(Line::from(spans));
        }

        lines
    }
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn style_role(role: &str) -> Style {
    match role {
        "user" => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        "assistant" => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        "action" => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        "output" => Style::default().fg(Color::Blue),
        "error" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        "system" => Style::default().fg(Color::Gray),
        _ => Style::default(),
    }
}

pub fn get_role_label(role: &str) -> &str {
    match role {
        "user" => "You",
        "assistant" => "codr",
        "action" => "Action",
        "output" => "Output",
        "error" => "Error",
        _ => role,
    }
}

pub fn visible_width(text: &str) -> usize {
    let stripped = strip_ansi(text);
    stripped.width()
}

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

pub struct StyledText {
    spans: Vec<Span<'static>>,
}

impl StyledText {
    pub fn new() -> Self {
        Self { spans: Vec::new() }
    }

    pub fn plain(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::raw(text.into()));
        self
    }

    pub fn bold(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(
            text.into(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        self
    }

    pub fn italic(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(
            text.into(),
            Style::default().add_modifier(Modifier::ITALIC),
        ));
        self
    }

    pub fn cyan(mut self, text: impl Into<String>) -> Self {
        self.spans
            .push(Span::styled(text.into(), Style::default().fg(Color::Cyan)));
        self
    }

    pub fn green(mut self, text: impl Into<String>) -> Self {
        self.spans
            .push(Span::styled(text.into(), Style::default().fg(Color::Green)));
        self
    }

    pub fn yellow(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(
            text.into(),
            Style::default().fg(Color::Yellow),
        ));
        self
    }

    pub fn red(mut self, text: impl Into<String>) -> Self {
        self.spans
            .push(Span::styled(text.into(), Style::default().fg(Color::Red)));
        self
    }

    pub fn blue(mut self, text: impl Into<String>) -> Self {
        self.spans
            .push(Span::styled(text.into(), Style::default().fg(Color::Blue)));
        self
    }

    pub fn gray(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(
            text.into(),
            Style::default().fg(Color::DarkGray),
        ));
        self
    }

    pub fn dim(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::styled(
            text.into(),
            Style::default().add_modifier(Modifier::DIM),
        ));
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

pub struct Keybinding {
    pub key: &'static str,
    pub description: &'static str,
}

pub const DEFAULT_KEYBINDINGS: &[Keybinding] = &[
    Keybinding {
        key: "Enter",
        description: "Send message",
    },
    Keybinding {
        key: "Ctrl+Q",
        description: "Quit",
    },
    Keybinding {
        key: "Ctrl+C",
        description: "Quit",
    },
    Keybinding {
        key: "a",
        description: "Approve action (when pending)",
    },
    Keybinding {
        key: "r",
        description: "Reject action (when pending)",
    },
    Keybinding {
        key: "Arrows",
        description: "Move cursor",
    },
];

pub fn render_keybindings() -> Vec<Line<'static>> {
    vec![StyledText::new()
        .gray("Keybindings: ")
        .dim("Enter: Send | Ctrl+Q: Quit | a: Approve | r: Reject")
        .into_line()]
}
