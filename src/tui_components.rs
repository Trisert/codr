use once_cell::sync::Lazy;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

// ── Theme ────────────────────────────────────────────────────

#[allow(dead_code)]
pub struct Theme {
    pub user: Style,
    pub assistant: Style,
    pub action: Style,
    pub output: Style,
    pub error: Style,
    pub system: Style,
    pub dim: Style,
    pub separator: Style,
    pub header: Style,
    pub prompt: Style,
    pub status: Style,
}

pub static THEME: Lazy<Theme> = Lazy::new(|| Theme {
    user: Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD),
    assistant: Style::default()
        .fg(Color::Rgb(180, 210, 255)),
    action: Style::default()
        .fg(Color::Rgb(255, 180, 100))
        .add_modifier(Modifier::BOLD),
    output: Style::default()
        .fg(Color::Rgb(140, 140, 140)),
    error: Style::default()
        .fg(Color::Rgb(255, 100, 100))
        .add_modifier(Modifier::BOLD),
    system: Style::default()
        .fg(Color::DarkGray),
    dim: Style::default()
        .fg(Color::DarkGray),
    separator: Style::default()
        .fg(Color::Rgb(60, 60, 60)),
    header: Style::default()
        .fg(Color::Rgb(100, 100, 120))
        .add_modifier(Modifier::BOLD),
    prompt: Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD),
    status: Style::default()
        .fg(Color::Rgb(80, 80, 100)),
});

// ── Approval / Pending ───────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum ApprovalState {
    None,
    Pending,
    Approved,
    Rejected,
}

#[derive(Clone, Debug)]
pub struct PendingAction {
    #[allow(dead_code)]
    pub action_type: String,
    pub content: String,
}

// ── ChatMessage ──────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[allow(dead_code)]
    pub thinking: Option<String>,
    pub timestamp: String,
}

impl ChatMessage {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
            thinking: None,
            timestamp: chrono_timestamp(),
        }
    }

    pub fn user(content: &str) -> Self { Self::new("user", content) }
    #[allow(dead_code)]
    pub fn assistant(content: &str) -> Self { Self::new("assistant", content) }

    pub fn assistant_with_thinking(content: &str) -> Self {
        let (clean_content, thinking) = extract_thinking(content);
        Self {
            role: "assistant".to_string(),
            content: clean_content,
            thinking,
            timestamp: chrono_timestamp(),
        }
    }

    pub fn system(content: &str) -> Self { Self::new("system", content) }
    pub fn action(content: &str) -> Self { Self::new("action", content) }
    pub fn output(content: &str) -> Self { Self::new("output", content) }
    pub fn error(content: &str) -> Self { Self::new("error", content) }
}

// ── Thinking Extraction ───────────────────────────────────────

fn extract_thinking(content: &str) -> (String, Option<String>) {
    // Support multiple thinking tag formats:
    // - Claude: <thinking>...</thinking>
    // - Qwen and others:
    let thinking_patterns = [
        ("<thinking>", "</thinking>"),
        ("
", ""),
    ];

    let mut clean_content = content.to_string();
    let mut thinking_content = None;

    // Extract thinking
    for (start_tag, end_tag) in thinking_patterns {
        if let Some(start) = clean_content.find(start_tag)
            && let Some(end) = clean_content.find(end_tag) {
                thinking_content = Some(clean_content[start + start_tag.len()..end].to_string());
                let before = clean_content[..start].to_string();
                let after = clean_content[end + end_tag.len()..].to_string();
                clean_content = format!("{}{}", before, after);
                break;
            }
    }

    // Remove tool-action blocks (```tool-action ... ```)
    clean_content = remove_code_blocks(&clean_content, "tool-action");

    // Remove bash-action blocks (```bash-action ... ```)
    clean_content = remove_code_blocks(&clean_content, "bash-action");

    (clean_content.trim().to_string(), thinking_content)
}

fn remove_code_blocks(content: &str, block_type: &str) -> String {
    let start_pattern = format!("```{}", block_type);
    let mut result = String::new();
    let mut remaining = content;

    while let Some(start) = remaining.find(&start_pattern) {
        // Keep everything before the block
        result.push_str(&remaining[..start]);

        // Find the end of the block
        let block_content = &remaining[start + start_pattern.len()..];
        if let Some(end) = block_content.find("```") {
            remaining = &block_content[end + 3..];
        } else {
            // No end found, skip the rest
            remaining = "";
            break;
        }
    }

    result.push_str(remaining);
    result.trim().to_string()
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

// ── Role styling helpers ─────────────────────────────────────

pub fn style_role(role: &str) -> Style {
    let t = &*THEME;
    match role {
        "user" => t.user,
        "assistant" => t.assistant,
        "action" => t.action,
        "output" => t.output,
        "error" => t.error,
        "system" => t.system,
        _ => Style::default(),
    }
}

pub fn role_prefix(role: &str) -> &'static str {
    match role {
        "user" => "> ",
        "assistant" => "",
        "action" => "",
        "output" => "  ",
        "error" => "! ",
        _ => "",
    }
}

#[allow(dead_code)]
pub fn get_role_label(role: &str) -> &str {
    match role {
        "user" => "You",
        "assistant" => "codr",
        "action" => "Tool",
        "output" => "Output",
        "error" => "Error",
        _ => role,
    }
}

// ── Render a single ChatMessage into display Lines ───────────

pub fn render_message(msg: &ChatMessage, width: usize) -> Vec<Line<'static>> {
    let t = &*THEME;
    let _style = style_role(&msg.role);
    let prefix = role_prefix(&msg.role);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let markdown = MarkdownRenderer::new();

    // Role header line (with timestamp for user / assistant)
    match msg.role.as_str() {
        "user" => {
            // User messages don't have a header, content is rendered below
        }
        "assistant" => {
            // Thinking content (if present) - displayed in italic
            if let Some(ref thinking) = msg.thinking {
                lines.push(Line::from("")); // newline before thinking

                let italic_style = Style::default().add_modifier(Modifier::ITALIC);
                let thinking_label = Style::default()
                    .fg(Color::Rgb(150, 150, 170))
                    .add_modifier(Modifier::ITALIC);

                let mut thinking_lines = thinking.lines().collect::<Vec<_>>();
                if !thinking_lines.is_empty() {
                    let first_line = thinking_lines.remove(0);
                    let wrapped = wrap_to_width(first_line, width.saturating_sub(14));
                    for (i, wrapped_line) in wrapped.into_iter().enumerate() {
                        if i == 0 {
                            lines.push(Line::from(vec![
                                Span::styled("  ", Style::default()),
                                Span::styled("Thinking: ", thinking_label),
                                Span::styled(wrapped_line, italic_style),
                            ]));
                        } else {
                            lines.push(Line::from(vec![
                                Span::raw("    ".to_string()),
                                Span::styled(wrapped_line, italic_style),
                            ]));
                        }
                    }

                    for line in thinking_lines {
                        let wrapped = wrap_to_width(line, width.saturating_sub(4));
                        for wrapped_line in wrapped {
                            lines.push(Line::from(vec![
                                Span::raw("    ".to_string()),
                                Span::styled(wrapped_line, italic_style),
                            ]));
                        }
                    }
                }
                lines.push(Line::from("")); // newline after thinking
            }
        }
        "action" => {
            lines.push(Line::from(vec![
                Span::styled(prefix.to_string(), t.action),
                Span::styled(msg.content.clone(), t.action),
            ]));
            return lines; // action is a single-line entry
        }
        "output" => {
            // output: indented, dimmed, no header
            for line in msg.content.lines() {
                let wrapped = wrap_to_width(line, width.saturating_sub(4));
                for wrapped_line in wrapped {
                    lines.push(Line::from(vec![
                        Span::styled("    ".to_string(), Style::default()),
                        Span::styled(wrapped_line, t.output),
                    ]));
                }
            }
            lines.push(Line::from("")); // newline after output
            return lines;
        }
        "error" => {
            lines.push(Line::from(vec![
                Span::styled(prefix.to_string(), t.error),
                Span::styled("Error".to_string(), t.error),
            ]));
        }
        _ => {}
    }

    // Content lines - use markdown rendering for assistant
    let content_lines = if msg.role == "assistant" {
        markdown.render(&msg.content)
    } else {
        vec![Line::from(Span::raw(msg.content.to_string()))]
    };

    for (i, md_line) in content_lines.into_iter().enumerate() {
        let line_prefix = if i == 0 { prefix } else { "  " };
        // Wrap each line to fit width
        let line_text = md_line.spans.iter().map(|s| s.content.to_string()).collect::<String>();
        let wrapped = wrap_to_width(&line_text, width.saturating_sub(2));
        for (j, wrapped_line) in wrapped.into_iter().enumerate() {
            let prefix = if i == 0 && j == 0 { line_prefix } else { "  " };
            lines.push(Line::from(vec![
                Span::raw(prefix.to_string()),
                Span::raw(wrapped_line),
            ]));
        }
    }

    // Blank line after each message block
    lines.push(Line::from(""));
    lines
}

// ── StatusLine ───────────────────────────────────────────────

#[allow(dead_code)]
pub struct StatusLine {
    pub model: String,
    pub tokens: u32,
    pub cost: f64,
    pub cwd: String,
}

#[allow(dead_code)]
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

// ── Markdown Renderer ────────────────────────────────────────

#[allow(dead_code)]
pub struct MarkdownRenderer {
    syntax_set: SyntaxSet,
    theme: syntect::highlighting::Theme,
}

#[allow(dead_code)]
impl MarkdownRenderer {
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme = THEME_SET.themes["base16-ocean.dark"].clone();
        Self { syntax_set, theme }
    }

    pub fn render(&self, text: &str) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut in_code_block = false;
        
        for line in text.lines() {
            // Check for fenced code block start/end
            if line.starts_with("```") {
                if in_code_block {
                    // End of code block
                    in_code_block = false;
                    lines.push(Line::from(Span::styled(
                        "└───────────────",
                        Style::default().fg(Color::Rgb(100, 100, 120)).add_modifier(Modifier::BOLD)
                    )));
                    continue;
                } else {
                    // Start of code block
                    in_code_block = true;
                    let lang = line[3..].trim().to_string();
                    lines.push(Line::from(Span::styled(
                        format!("┌─ {} ─", lang),
                        Style::default().fg(Color::Rgb(100, 100, 120)).add_modifier(Modifier::BOLD)
                    )));
                    continue;
                }
            }
            
            if in_code_block {
                // Render code block line
                lines.push(Line::from(vec![
                    Span::styled("│ ".to_string(), Style::default().fg(Color::Rgb(100, 100, 120))),
                    Span::styled(line.to_string(), Style::default().fg(Color::Rgb(200, 180, 150))),
                ]));
            } else {
                let processed = self.process_markdown_line(line);
                lines.push(processed);
            }
        }
        
        lines
    }

    fn process_markdown_line(&self, line: &str) -> Line<'static> {
        if let Some(stripped) = line.strip_prefix("# ") {
            return Line::from(vec![Span::styled(
                stripped.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED),
            )]);
        }
        if let Some(stripped) = line.strip_prefix("## ") {
            return Line::from(vec![Span::styled(
                stripped.to_string(),
                Style::default()
                    .fg(Color::Rgb(180, 210, 255))
                    .add_modifier(Modifier::BOLD),
            )]);
        }
        if let Some(stripped) = line.strip_prefix("### ") {
            return Line::from(vec![Span::styled(
                stripped.to_string(),
                Style::default()
                    .fg(Color::Rgb(200, 170, 255))
                    .add_modifier(Modifier::BOLD),
            )]);
        }

        if line.starts_with("- ") || line.starts_with("* ") {
            return Line::from(vec![
                Span::styled("  * ".to_string(), Style::default().fg(Color::Rgb(255, 180, 100))),
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

// ── Text utilities ───────────────────────────────────────────

#[allow(dead_code)]
pub fn visible_width(text: &str) -> usize {
    let stripped = strip_ansi(text);
    stripped.width()
}

#[allow(dead_code)]
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

pub fn wrap_to_width(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![];
    }
    
    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width: usize = word.chars().map(|c| c.width().unwrap_or(0)).sum();
        
        if current_width + word_width + (if current_width > 0 { 1 } else { 0 }) > width
            && !current_line.is_empty()
        {
            lines.push(current_line.clone());
            current_line.clear();
            current_width = 0;
        }
        
        if current_width > 0 {
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

// ── Keybindings ──────────────────────────────────────────────

pub fn render_hint_line(approval_pending: bool) -> Line<'static> {
    let t = &*THEME;
    if approval_pending {
        Line::from(vec![
            Span::styled("a", Style::default().fg(Color::Rgb(120, 220, 120)).add_modifier(Modifier::BOLD)),
            Span::styled(" approve ", t.dim),
            Span::styled("r", Style::default().fg(Color::Rgb(255, 100, 100)).add_modifier(Modifier::BOLD)),
            Span::styled(" reject", t.dim),
        ])
    } else {
        Line::from(vec![
            Span::styled("enter", t.dim),
            Span::styled(" send ", t.dim),
            Span::styled("ctrl+c", t.dim),
            Span::styled(" quit ", t.dim),
        ])
    }
}
