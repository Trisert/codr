use once_cell::sync::Lazy;
use ratatui::{
    prelude::Stylize,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::sync::Arc;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

// ── Color Blending Helpers (codr Style) ──────────────────────
fn is_light(bg: (u8, u8, u8)) -> bool {
    let (r, g, b) = bg;
    let y = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    y > 128.0
}

fn blend(fg: (u8, u8, u8), bg: (u8, u8, u8), alpha: f32) -> (u8, u8, u8) {
    let r = (fg.0 as f32 * alpha + bg.0 as f32 * (1.0 - alpha)) as u8;
    let g = (fg.1 as f32 * alpha + bg.1 as f32 * (1.0 - alpha)) as u8;
    let b = (fg.2 as f32 * alpha + bg.2 as f32 * (1.0 - alpha)) as u8;
    (r, g, b)
}

pub fn get_user_message_bg(terminal_bg: (u8, u8, u8)) -> Color {
    let (top, alpha) = if is_light(terminal_bg) {
        ((0, 0, 0), 0.04)
    } else {
        ((255, 255, 255), 0.12)
    };
    let blended = blend(top, terminal_bg, alpha);
    Color::Rgb(blended.0, blended.1, blended.2)
}

// ── Theme ────────────────────────────────────────────────────

#[allow(dead_code)]
pub struct Theme {
    pub user: Style,
    pub assistant: Style,
    pub action: Style,
    pub output: Style,
    pub error: Style,
    pub system: Style,
    pub info: Style,
    pub dim: Style,
    pub separator: Style,
    pub header: Style,
    pub prompt: Style,
    pub status: Style,
}

pub static THEME: Lazy<Theme> = Lazy::new(|| {
    // We assume a standard dark terminal background (e.g., 30, 30, 30) for the blended background,
    // since we can't cleanly query actual terminal BG sync here without a termcap/crossterm query wrapper.
    let user_bg = get_user_message_bg((30, 30, 30));

    Theme {
        // User messages: distinctive background with bright text
        user: Style::default()
            .fg(Color::Rgb(252, 252, 252)) // Crisp white
            .bg(user_bg),
        // Assistant: soft, readable gray with slight blue tint
        assistant: Style::default()
            .fg(Color::Rgb(226, 226, 232)), // Soft white with blue tint
        // Action: warm amber for better visibility
        action: Style::default()
            .fg(Color::Rgb(255, 200, 100)) // Bright amber/gold
            .add_modifier(Modifier::ITALIC),
        // Output: subtle but visible
        output: Style::default().fg(Color::Rgb(163, 165, 170)), // Medium gray
        // Error: urgent red
        error: Style::default()
            .fg(Color::Rgb(248, 113, 113)) // Bright red
            .add_modifier(Modifier::BOLD),
        // System: very subtle
        system: Style::default().fg(Color::Rgb(82, 82, 86)),
        // Info: calm teal
        info: Style::default()
            .fg(Color::Rgb(103, 232, 189)) // Teal/cyan
            .add_modifier(Modifier::ITALIC),
        // Dim: for subtle text
        dim: Style::default().fg(Color::Rgb(92, 92, 97)),
        // Separator: subtle divider
        separator: Style::default().fg(Color::Rgb(46, 46, 48)),
        // Header: for emphasis
        header: Style::default()
            .fg(Color::Rgb(203, 213, 245)) // Soft blue-white
            .add_modifier(Modifier::BOLD),
        // Prompt: inviting blue
        prompt: Style::default()
            .fg(Color::Rgb(147, 197, 253)) // Sky blue
            .add_modifier(Modifier::BOLD),
        // Status: subtle footer
        status: Style::default().fg(Color::Rgb(115, 115, 120)),
    }
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
    pub action_type: Arc<str>,  // Shared, immutable
    pub content: Arc<String>,  // Shared command content
}

// ── ChatMessage ──────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: Arc<str>,  // Shared, immutable (e.g., "user", "assistant")
    pub content: Arc<String>,  // Shared, potentially large content
    #[allow(dead_code)]
    pub thinking: Option<String>,
    #[allow(dead_code)]
    pub timestamp: String,
}

impl ChatMessage {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.into(),
            content: Arc::new(content.to_string()),
            thinking: None,
            timestamp: chrono_timestamp(),
        }
    }

    pub fn user(content: &str) -> Self {
        Self::new("user", content)
    }
    #[allow(dead_code)]
    pub fn assistant(content: &str) -> Self {
        let cleaned = clean_tool_tags(content);
        Self::new("assistant", &cleaned)
    }

    /// Create an assistant message with explicit thinking content
    pub fn assistant_with_explicit_thinking(content: &str, thinking: Option<String>) -> Self {
        let cleaned = clean_tool_tags(content);
        Self {
            role: "assistant".into(),
            content: Arc::new(cleaned),
            thinking,
            timestamp: chrono_timestamp(),
        }
    }

    #[allow(dead_code)]
    pub fn assistant_with_thinking(content: &str) -> Self {
        let (clean_content, thinking) = extract_thinking(content);
        Self {
            role: "assistant".into(),
            content: Arc::new(clean_content),
            thinking,
            timestamp: chrono_timestamp(),
        }
    }

    pub fn system(content: &str) -> Self {
        Self::new("system", content)
    }
    pub fn action(content: &str) -> Self {
        // Don't minify action content - it's already formatted (e.g., "bash: command")
        // minify_xml is only needed for raw XML from LLM
        Self::new("action", content)
    }
    pub fn output(content: &str) -> Self {
        Self::new("output", content)
    }
    pub fn info(content: &str) -> Self {
        Self::new("info", content)
    }
    pub fn error(content: &str) -> Self {
        Self::new("error", content)
    }
}

// ── Content Cleaning ───────────────────────────────────────────

/// Remove codr tool call tags and thinking tags from content before displaying
fn clean_tool_tags(content: &str) -> String {
    let mut result = content.to_string();

    // Remove <codr_tool>...</codr_tool> tags
    while let Some(start) = result.find("<codr_tool") {
        if let Some(end_tag) = result[start..].find("</codr_tool>") {
            let end = start + end_tag + "</codr_tool>".len();
            result.replace_range(start..end, "");
        } else {
            // No closing tag, remove from start to end
            result.truncate(start);
            break;
        }
    }

    // Remove <codr_bash>...</codr_bash> tags
    while let Some(start) = result.find("<codr_bash>") {
        if let Some(end_tag) = result[start..].find("</codr_bash>") {
            let end = start + end_tag + "</codr_bash>".len();
            result.replace_range(start..end, "");
        } else {
            result.truncate(start);
            break;
        }
    }

    // Remove thinking tags
    let thinking_tags = [("<thinking>", "</thinking>"), ("<think>", "</think>")];
    for (start_tag, end_tag) in thinking_tags {
        while let Some(start) = result.find(start_tag) {
            if let Some(end_offset) = result[start..].find(end_tag) {
                let end = start + end_offset + end_tag.len();
                result.replace_range(start..end, "");
            } else {
                result.truncate(start);
                break;
            }
        }
    }

    result.trim().to_string()
}

/// Clean content for conversation history (remove XML tags but preserve semantic meaning)
/// This is called when adding LLM output to the conversation history for the next turn
pub fn clean_for_conversation(content: &str) -> String {
    let mut result = content.to_string();

    // Remove <codr_tool name="XXX">params</codr_tool> tags entirely
    while let Some(start) = result.find("<codr_tool") {
        if let Some(end_tag) = result[start..].find("</codr_tool>") {
            let end = start + end_tag + "</codr_tool>".len();
            result.replace_range(start..end, "");
        } else {
            result.truncate(start);
            break;
        }
    }

    // Remove <codr_bash>command</codr_bash> tags entirely
    while let Some(start) = result.find("<codr_bash>") {
        if let Some(end_tag) = result[start..].find("</codr_bash>") {
            let end = start + end_tag + "</codr_bash>".len();
            result.replace_range(start..end, "");
        } else {
            result.truncate(start);
            break;
        }
    }

    // Remove thinking tags entirely
    let thinking_tags = [("<thinking>", "</thinking>"), ("<think>", "</think>")];
    for (start_tag, end_tag) in thinking_tags {
        while let Some(start) = result.find(start_tag) {
            if let Some(end_offset) = result[start..].find(end_tag) {
                let end = start + end_offset + end_tag.len();
                result.replace_range(start..end, "");
            } else {
                result.truncate(start);
                break;
            }
        }
    }

    // Clean up extra whitespace that might result from tag removal
    while result.contains("\n\n\n") {
        result = result.replace("\n\n\n", "\n\n");
    }

    result.trim().to_string()
}

// ── Thinking Extraction ───────────────────────────────────────

#[allow(dead_code)]
fn extract_thinking(content: &str) -> (String, Option<String>) {
    // Support multiple thinking tag formats:
    // - Claude: <thinking>...</thinking>
    // - Qwen and others:
    let thinking_patterns = [
        ("<thinking>", "</thinking>"),
        ("<think>", "</think>"),
    ];

    let mut clean_content = content.to_string();
    let mut thinking_content = None;

    // Extract thinking
    for (start_tag, end_tag) in thinking_patterns {
        if let Some(start) = clean_content.find(start_tag)
            && let Some(end) = clean_content.find(end_tag)
            && start < end
        {
            thinking_content = Some(clean_content[start + start_tag.len()..end].to_string());
            let before = clean_content[..start].to_string();
            let after = clean_content[end + end_tag.len()..].to_string();
            clean_content = format!("{}{}", before, after);
            break;
        }
    }

    (clean_content.trim().to_string(), thinking_content)
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
        "info" => t.info,
        _ => Style::default(),
    }
}

pub fn role_prefix(role: &str) -> &'static str {
    match role {
        "user" => "",
        "assistant" => "▪ ",
        "action" => "",
        "output" => "  ",
        "error" => "✕ ",
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

// ── Spacing System (following OpenCode design) ──────────────────
// Tight: 0 lines (consecutive related items like tool calls)
// Normal: 1 line (between different message types)
// Loose: 2 lines (before user exchanges, major sections)
// ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Spacing {
    Tight,   // 0 blank lines - for consecutive tool/info messages
    Normal,  // 1 blank line - between different message types
    None,    // No spacing - for first message or compact display
}

// ── Render a single ChatMessage into display Lines ───────────

pub fn render_message(msg: &ChatMessage, width: usize) -> Vec<Line<'_>> {
    let t = &*THEME;
    let _style = style_role(&msg.role);
    let prefix = role_prefix(msg.role.as_ref());
    let mut lines: Vec<Line<'_>> = Vec::new();
    let markdown = MarkdownRenderer::new();

    // Determine spacing based on role (following OpenCode's gap system)
    let spacing = match &*msg.role {
        "user" => Spacing::Normal,       // 1 line - half blank before user
        "assistant" => Spacing::Normal,   // 1 line - half blank after thinking
        "action" => Spacing::Tight,      // 0 lines - no gap between sequential tool calls
        "info" => Spacing::Tight,        // 0 lines - no gap between sequential tool calls
        "output" => Spacing::Tight,      // 0 lines - no gap between action and output
        "error" => Spacing::Normal,       // 1 line before error
        _ => Spacing::None,
    };

    // Apply top spacing
    match spacing {
        Spacing::Normal => {
            lines.push(Line::from(""));
        }
        Spacing::Tight | Spacing::None => {}
    }

    // Render message content
    match &*msg.role {
        "assistant" => {
            // Thinking content (if present) - displayed in italic
            if let Some(ref thinking) = msg.thinking {
                let italic_style =
                    Style::default()
                        .add_modifier(Modifier::ITALIC)
                        .fg(Color::Rgb(163, 165, 170));

                let mut thinking_lines = thinking.lines().collect::<Vec<_>>();
                if !thinking_lines.is_empty() {
                    let first_line = thinking_lines.remove(0);
                    let wrapped = wrap_to_width(first_line, width.saturating_sub(14));
                    for (i, wrapped_line) in wrapped.into_iter().enumerate() {
                        if i == 0 {
                            lines.push(Line::from(vec![
                                Span::styled("▪ ", t.dim),
                                Span::styled("Thinking: ", Style::default().fg(Color::Rgb(92, 92, 97)).add_modifier(Modifier::BOLD | Modifier::ITALIC)),
                                Span::styled(wrapped_line, italic_style),
                            ]));
                        } else {
                            lines.push(Line::from(vec![
                                Span::raw("  "),
                                Span::styled(wrapped_line, italic_style),
                            ]));
                        }
                    }

                    for line in thinking_lines {
                        let wrapped = wrap_to_width(line, width.saturating_sub(4));
                        for wrapped_line in wrapped {
                            lines.push(Line::from(vec![
                                Span::raw("  "),
                                Span::styled(wrapped_line, italic_style),
                            ]));
                        }
                    }
                }
                // Add spacing after thinking
                lines.push(Line::from(""));
            }
        }
        "action" => {
            lines.push(Line::from(vec![
                Span::styled(prefix.to_string(), t.action),
                Span::styled(&*msg.content, t.action),
            ]));
            return lines;
        }
        "output" => {
            // Check if this looks like file content
            let has_code_patterns = msg.content.contains("fn ")
                || msg.content.contains("struct ")
                || msg.content.contains("impl ")
                || msg.content.contains("use ")
                || msg.content.contains("mod ")
                || msg.content.contains("pub ")
                || msg.content.contains("let ")
                || msg.content.contains("async ")
                || msg.content.contains("await ")
                || msg.content.contains("  mut ")
                || msg.content.contains("//")
                || msg.content.contains("/*")
                || msg.content.contains("#!");
            let looks_like_command_output = msg.content.contains("total ")
                || msg.content.starts_with("drwx")
                || msg.content.starts_with("-rw")
                || msg.content.contains("command not found")
                || msg.content.contains("No such file")
                || msg.content.lines().count() <= 3;
            let is_file_content = has_code_patterns && !looks_like_command_output;

            if is_file_content {
                // File content: clean, indented display
                for line in msg.content.lines() {
                    let wrapped = wrap_to_width(line, width.saturating_sub(4));
                    for wrapped_line in wrapped {
                        lines.push(Line::from(vec![Span::styled(
                            format!("  {}", wrapped_line),
                            Style::default().fg(Color::Rgb(203, 213, 245)),
                        )]));
                    }
                }
            } else {
                // Command output: subtle background for better visibility
                let output_style =
                    Style::default().fg(Color::Rgb(226, 232, 240)).bg(Color::Rgb(38, 38, 42));
                for line in msg.content.lines() {
                    let wrapped = wrap_to_width(line, width.saturating_sub(4));
                    for wrapped_line in wrapped {
                        let current_width = wrapped_line.width();
                        let padding = width.saturating_sub(current_width + 2);
                        lines.push(Line::from(vec![
                            Span::styled("  ", output_style),
                            Span::styled(wrapped_line, output_style),
                            Span::styled(" ".repeat(padding), output_style),
                        ]));
                    }
                }
            }
            return lines;
        }
        "error" => {
            lines.push(Line::from(vec![
                Span::styled("✕ ", t.error),
                Span::styled(&*msg.content, t.error),
            ]));
            lines.push(Line::from(""));
            return lines;
        }
        "info" => {
            lines.push(Line::from(vec![
                Span::styled("-> ", t.info),
                Span::styled(&*msg.content, t.info),
            ]));
            return lines;
        }
        _ => {}
    }

    // Render message content with padding
    let content_lines = if &*msg.role == "assistant" {
        markdown.render_with_width(&msg.content, width.saturating_sub(2))
    } else {
        wrap_to_width(&msg.content, width.saturating_sub(3))
            .into_iter()
            .map(|w| Line::from(Span::raw(w)))
            .collect()
    };

    // Note: style is now applied inline per role type

    for (i, md_line) in content_lines.into_iter().enumerate() {
        if &*msg.role == "user" {
            // User messages: simple ">" prefix
            let prefix = "> ";
            let mut line_spans = vec![
                Span::styled(prefix, Style::default().fg(Color::Rgb(147, 197, 253)).add_modifier(Modifier::BOLD)),
            ];

            // Add content spans with soft white color
            line_spans.extend(md_line.spans.into_iter().map(|mut s| {
                s.style = s.style.fg(Color::Rgb(226, 232, 240));
                s
            }));

            lines.push(Line::from(line_spans));
        } else if &*msg.role == "assistant" {
            // Assistants get the `▪ ` prefix
            let line_prefix = if i == 0 { "▪ " } else { "  " };
            let mut line_spans = vec![
                if i == 0 { Span::styled(line_prefix, t.dim.add_modifier(Modifier::BOLD)) } else { Span::raw(line_prefix) }
            ];
            line_spans.extend(md_line.spans);
            lines.push(Line::from(line_spans));
        } else {
            let mut line_spans = vec![Span::raw("  ")];
            line_spans.extend(md_line.spans);
            lines.push(Line::from(line_spans));
        }
    }

    // No bottom spacing - let the next message's top spacing handle it
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

/// Inline markdown element
#[derive(Clone, Debug)]
enum InlineElement {
    Text(String),
    Bold(String),
    Italic(String),
    Strikethrough(String),
    Code(String),
    Link { text: String, _url: String },
}

/// Table cell data
#[derive(Clone, Debug)]
struct TableCell {
    content: String,
    alignment: TableAlignment,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TableAlignment {
    Left,
    Center,
    Right,
    Unspecified,
}

/// Table data structure
#[derive(Clone, Debug)]
struct Table {
    headers: Vec<TableCell>,
    rows: Vec<Vec<TableCell>>,
}

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

    /// Main render method - processes markdown and returns styled lines
    pub fn render(&self, text: &str) -> Vec<Line<'static>> {
        self.render_with_width(text, usize::MAX)
    }

    /// Render with width constraint for proper wrapping
    pub fn render_with_width(&self, text: &str, width: usize) -> Vec<Line<'static>> {
        let mut result = Vec::new();
        let lines_vec: Vec<&str> = text.lines().collect();
        let mut i = 0;

        while i < lines_vec.len() {
            let line = lines_vec[i];

            // Check for fenced code block start
            if let Some(rest) = line.strip_prefix("```") {
                let lang = rest.trim().to_string();
                let mut code_content = String::new();
                i += 1;

                // Collect code content until closing fence
                while i < lines_vec.len() {
                    if lines_vec[i].starts_with("```") {
                        i += 1;
                        break;
                    }
                    code_content.push_str(lines_vec[i]);
                    code_content.push('\n');
                    i += 1;
                }

                // Render code block with syntax highlighting
                result.extend(self.render_code_block(&code_content, &lang));
                continue;
            }

            // Check for horizontal rule
            if self.is_horizontal_rule(line) {
                result.push(self.render_horizontal_rule());
                i += 1;
                continue;
            }

            // Check for table (look ahead for separator line)
            if self.is_table_header(line)
                && i + 1 < lines_vec.len()
                && self.is_table_separator(lines_vec[i + 1])
            {
                let table = self.parse_table(&lines_vec, i);
                if let Some((table_data, lines_consumed)) = table {
                    result.extend(self.render_table(&table_data));
                    i += lines_consumed;
                    continue;
                }
            }

            // Regular line - process as markdown and wrap if needed
            let rendered = self.process_markdown_line(line);

            // Wrap the rendered line if it exceeds width
            if width != usize::MAX {
                for wrapped_line in self.wrap_line(&rendered, width) {
                    result.push(wrapped_line);
                }
            } else {
                result.push(rendered);
            }
            i += 1;
        }

        result
    }

    /// Wrap a single line to fit within the given width
    fn wrap_line(&self, line: &Line, width: usize) -> Vec<Line<'static>> {
        let mut result = Vec::new();

        // Convert spans to plain text, explicitly collecting each span's content
        let mut plain_text = String::new();
        for span in &line.spans {
            plain_text.push_str(span.content.as_ref());
        }

        if plain_text.width() <= width {
            // Line fits, return as-is (convert to owned)
            let owned_spans: Vec<Span> = line
                .spans
                .iter()
                .map(|s| Span::styled(s.content.to_string(), s.style))
                .collect();
            result.push(Line::from(owned_spans));
        } else {
            // Line is too long, wrap it using wrap_to_width which preserves spaces
            let wrapped = wrap_to_width(&plain_text, width);
            for wrapped_line in wrapped {
                // Preserve the style from the original spans for wrapped text
                let style = line.spans.first().map(|s| s.style).unwrap_or_default();
                result.push(Line::from(Span::styled(wrapped_line, style)));
            }
        }

        result
    }

    /// Check if a line is a horizontal rule
    fn is_horizontal_rule(&self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.len() < 3 {
            return false;
        }

        let first_char = trimmed.chars().next().unwrap();
        if !matches!(first_char, '-' | '*' | '_') {
            return false;
        }

        trimmed
            .chars()
            .all(|c| c == first_char || c.is_whitespace())
    }

    /// Render a horizontal rule
    fn render_horizontal_rule(&self) -> Line<'static> {
        Line::from(Span::styled(
            "────────────────────────────────────────",
            Style::default().fg(Color::Rgb(80, 80, 100)),
        ))
    }

    /// Check if a line is a table header (contains pipes)
    fn is_table_header(&self, line: &str) -> bool {
        line.trim().starts_with('|')
            && line.contains('|')
            && line.chars().filter(|&c| c == '|').count() >= 2
    }

    /// Check if a line is a table separator (contains dashes, pipes, and colons for alignment)
    fn is_table_separator(&self, line: &str) -> bool {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            return false;
        }
        // Check if it looks like: |---|:---:|---|
        let parts: Vec<&str> = trimmed.split('|').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return false;
        }
        parts.iter().all(|p| {
            let p = p.trim();
            p.len() >= 3 && p.chars().all(|c| c == '-' || c == ':')
                || p == "---"
                || p == ":---"
                || p == ":---:"
                || p == "---:"
        })
    }

    /// Parse a table from lines starting at index i
    fn parse_table(&self, lines: &[&str], start_idx: usize) -> Option<(Table, usize)> {
        if start_idx >= lines.len() {
            return None;
        }

        // Parse header
        let header_line = lines[start_idx].trim();
        let headers: Vec<TableCell> = header_line
            .split('|')
            .skip(1) // Skip empty string before first |
            .filter_map(|s| {
                let s = s.trim();
                if !s.is_empty() {
                    Some(TableCell {
                        content: s.to_string(),
                        alignment: TableAlignment::Unspecified,
                    })
                } else {
                    None
                }
            })
            .collect();

        if headers.is_empty() {
            return None;
        }

        // Parse separator to get alignment
        if start_idx + 1 >= lines.len() {
            return None;
        }
        let sep_line = lines[start_idx + 1].trim();
        let alignments: Vec<TableAlignment> = sep_line
            .split('|')
            .skip(1)
            .map(|s| {
                let s = s.trim();
                if s.starts_with(':') && s.ends_with(':') {
                    TableAlignment::Center
                } else if s.ends_with(':') {
                    TableAlignment::Right
                } else if s.starts_with(':') {
                    TableAlignment::Center // :--- is usually left or center, we'll use left
                } else {
                    TableAlignment::Left
                }
            })
            .collect();

        // Apply alignments to headers
        let headers_with_align: Vec<TableCell> = headers
            .iter()
            .enumerate()
            .map(|(i, cell)| TableCell {
                content: cell.content.clone(),
                alignment: alignments
                    .get(i)
                    .copied()
                    .unwrap_or(TableAlignment::Unspecified),
            })
            .collect();

        // Parse rows
        let mut rows = Vec::new();
        let mut i = start_idx + 2;
        while i < lines.len() {
            let line = lines[i].trim();
            if !line.starts_with('|') || line.chars().filter(|&c| c == '|').count() < 2 {
                break;
            }

            let cells: Vec<TableCell> = line
                .split('|')
                .skip(1)
                .enumerate()
                .filter_map(|(idx, s)| {
                    let s = s.trim();
                    if !s.is_empty() || idx < headers_with_align.len() {
                        Some(TableCell {
                            content: s.to_string(),
                            alignment: alignments
                                .get(idx)
                                .copied()
                                .unwrap_or(TableAlignment::Unspecified),
                        })
                    } else {
                        None
                    }
                })
                .collect();

            if !cells.is_empty() {
                rows.push(cells);
            }
            i += 1;
        }

        Some((
            Table {
                headers: headers_with_align,
                rows,
            },
            i - start_idx,
        ))
    }

    /// Render a table
    fn render_table(&self, table: &Table) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        if table.headers.is_empty() {
            return lines;
        }

        // Calculate column widths
        let num_cols = table.headers.len();
        let mut col_widths = vec![0usize; num_cols];

        for (idx, header) in table.headers.iter().enumerate() {
            col_widths[idx] = col_widths[idx].max(visible_width(&header.content));
        }

        for row in &table.rows {
            for (idx, cell) in row.iter().enumerate() {
                if idx < col_widths.len() {
                    col_widths[idx] = col_widths[idx].max(visible_width(&cell.content));
                }
            }
        }

        // Add minimum width for aesthetics
        for width in &mut col_widths {
            *width = (*width).max(3);
        }

        let border_color = Color::Rgb(82, 82, 92);
        let header_style = Style::default()
            .fg(Color::Rgb(203, 213, 245)) // Soft blue-white
            .add_modifier(Modifier::BOLD);
        let separator_style = Style::default().fg(border_color);

        // Add spacing before table
        lines.push(Line::from(""));

        // Render header separator line first
        let mut sep_spans = vec![Span::styled("┌", separator_style)];
        for (idx, width) in col_widths.iter().enumerate() {
            sep_spans.push(Span::styled(
                "─".repeat(*width + 2),
                separator_style,
            ));
            if idx < col_widths.len() - 1 {
                sep_spans.push(Span::styled("┬", separator_style));
            } else {
                sep_spans.push(Span::styled("┐", separator_style));
            }
        }
        lines.push(Line::from(sep_spans));

        // Render header row
        let mut header_spans = vec![Span::styled("│ ", separator_style)];
        for (idx, header) in table.headers.iter().enumerate() {
            let width = col_widths[idx];
            // Truncate header if needed (prevents wrapping)
            let content = if header.content.width() > width {
                let mut truncated = header.content.chars().take(width.saturating_sub(3)).collect::<String>();
                if header.content.width() > width {
                    truncated.push_str("...");
                }
                truncated
            } else {
                header.content.clone()
            };
            let aligned = align_text(&content, width, header.alignment);
            header_spans.push(Span::styled(aligned, header_style));
            header_spans.push(Span::styled(" │ ", separator_style));
        }
        lines.push(Line::from(header_spans));

        // Render separator after header
        let mut sep_spans = vec![Span::styled("├", separator_style)];
        for (idx, header) in table.headers.iter().enumerate() {
            let align = header.alignment;
            let _left = matches!(align, TableAlignment::Right | TableAlignment::Center);
            let _right = matches!(align, TableAlignment::Left | TableAlignment::Center);

            sep_spans.push(Span::styled(
                format!("{}{}{}", "─", "─".repeat(col_widths[idx]), "─"),
                separator_style,
            ));
            if idx < col_widths.len() - 1 {
                sep_spans.push(Span::styled("┼", separator_style));
            } else {
                sep_spans.push(Span::styled("┤", separator_style));
            }
        }
        lines.push(Line::from(sep_spans));

        // Render data rows
        for row in &table.rows {
            let mut row_spans = vec![Span::styled("│ ", separator_style)];
            for (idx, cell) in row.iter().enumerate() {
                if idx < col_widths.len() {
                    let width = col_widths[idx];
                    // Truncate content if it exceeds column width (prevents wrapping)
                    let content = if cell.content.width() > width {
                        let mut truncated = cell.content.chars().take(width.saturating_sub(3)).collect::<String>();
                        if cell.content.width() > width {
                            truncated.push_str("...");
                        }
                        truncated
                    } else {
                        cell.content.clone()
                    };
                    let aligned = align_text(&content, width, cell.alignment);
                    // Don't process inline markdown in tables - it breaks borders
                    // Just use the aligned text with default style
                    row_spans.push(Span::styled(aligned, Style::default().fg(Color::Rgb(220, 220, 220))));
                    row_spans.push(Span::styled(" │ ", separator_style));
                }
            }
            lines.push(Line::from(row_spans));
        }

        // Render bottom border
        let mut bottom_spans = vec![Span::styled("└", separator_style)];
        for (idx, width) in col_widths.iter().enumerate() {
            bottom_spans.push(Span::styled(
                "─".repeat(*width + 2),
                separator_style,
            ));
            if idx < col_widths.len() - 1 {
                bottom_spans.push(Span::styled("┴", separator_style));
            } else {
                bottom_spans.push(Span::styled("┘", separator_style));
            }
        }
        lines.push(Line::from(bottom_spans));

        // Add spacing after table
        lines.push(Line::from(""));

        lines
    }

    /// Render a code block with syntax highlighting (no borders/backticks)
    fn render_code_block(&self, code: &str, language: &str) -> Vec<Line<'static>> {
        let bg_color = Color::Rgb(30, 30, 35);
        let bg_style = Style::default().bg(bg_color);

        let mut result = Vec::new();

        // Add subtle spacing before code block
        result.push(Line::from(""));

        // Syntax-highlighted content with background
        let highlighted = self.highlight_code(code, language);
        for line in highlighted {
            let mut spans = Vec::new();
            spans.extend(line.spans.into_iter().map(|mut s| {
                s.style = s.style.bg(bg_color);
                s
            }));
            result.push(Line::from(spans).style(bg_style));
        }

        // Add subtle spacing after code block
        result.push(Line::from(""));

        result
    }

    /// Process a single markdown line into a styled Line
    fn process_markdown_line(&self, line: &str) -> Line<'static> {
        let trimmed = line.trim_start();

        // Headers - support up to level 6
        if let Some(level) = self.get_header_level(trimmed) {
            return self.render_header(trimmed, level);
        }

        // Blockquotes
        if let Some(stripped) = trimmed.strip_prefix("> ") {
            return self.render_blockquote(stripped);
        }

        // Unordered lists
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            return self.render_list_item(&trimmed[2..], false);
        }

        // Ordered lists
        if let Some(content) = self.parse_ordered_list(trimmed) {
            return self.render_list_item(&content, true);
        }

        // Task lists
        if let Some((checked, content)) = self.parse_task_list(trimmed) {
            return self.render_task_item(checked, content);
        }

        // Regular line with inline markdown
        self.render_inline_line(line)
    }

    /// Get header level (1-6) or None
    fn get_header_level(&self, line: &str) -> Option<u8> {
        if line.starts_with("###### ") {
            Some(6)
        } else if line.starts_with("##### ") {
            Some(5)
        } else if line.starts_with("#### ") {
            Some(4)
        } else if line.starts_with("### ") {
            Some(3)
        } else if line.starts_with("## ") {
            Some(2)
        } else if line.starts_with("# ") {
            Some(1)
        } else {
            None
        }
    }

    /// Render a header
    fn render_header(&self, line: &str, level: u8) -> Line<'static> {
        let content = &line[level as usize..].trim_start();
        let (color, modifier) = match level {
            1 => (Color::Cyan, Modifier::BOLD | Modifier::UNDERLINED),
            2 => (Color::Rgb(180, 210, 255), Modifier::BOLD),
            3 => (Color::Rgb(200, 170, 255), Modifier::BOLD),
            4 => (Color::Rgb(220, 190, 255), Modifier::BOLD),
            5 => (Color::Rgb(240, 210, 255), Modifier::BOLD),
            6 => (Color::Rgb(240, 230, 255), Modifier::BOLD),
            _ => (Color::Rgb(180, 210, 255), Modifier::BOLD),
        };

        Line::from(vec![Span::styled(
            content.to_string(),
            Style::default().fg(color).add_modifier(modifier),
        )])
    }

    /// Render a blockquote
    fn render_blockquote(&self, content: &str) -> Line<'static> {
        let processed = self.render_inline_span(
            content,
            Style::default().fg(Color::Rgb(180, 180, 180)).italic(),
        );
        let mut line = Line::from(Span::styled("│ ", Style::default().fg(Color::Rgb(150, 150, 150))));
        line.spans.extend(processed.spans);
        line
    }

    /// Render a list item
    fn render_list_item(&self, content: &str, _ordered: bool) -> Line<'static> {
        let bullet = Span::styled("• ", Style::default().fg(Color::Rgb(255, 180, 100)));
        let processed = self.render_inline_span(content, Style::default());
        let mut line = Line::from(bullet);
        line.spans.extend(processed.spans);
        line
    }

    /// Parse an ordered list item
    fn parse_ordered_list(&self, line: &str) -> Option<String> {
        if !line.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return None;
        }
        if let Some(dot_pos) = line.find('.')
            && dot_pos < 5
            && line.chars().take(dot_pos).all(|c| c.is_ascii_digit())
        {
            let rest = line[dot_pos + 1..].trim_start();
            return Some(rest.to_string());
        }
        None
    }

    /// Parse a task list item
    fn parse_task_list(&self, line: &str) -> Option<(bool, String)> {
        let patterns = [
            ("* [x] ", true),
            ("* [X] ", true),
            ("- [x] ", true),
            ("- [X] ", true),
            ("* [ ] ", false),
            ("- [ ] ", false),
        ];

        for (prefix, checked) in patterns {
            if let Some(content) = line.strip_prefix(prefix) {
                return Some((checked, content.to_string()));
            }
        }
        None
    }

    /// Render a task list item
    fn render_task_item(&self, checked: bool, content: String) -> Line<'static> {
        let checkbox = if checked {
            Span::styled("[✓] ", Style::default().fg(Color::Rgb(100, 200, 100)))
        } else {
            Span::styled("[ ] ", Style::default().fg(Color::Rgb(150, 150, 150)))
        };

        let style = if checked {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            Style::default()
        };

        let processed = self.render_inline_span(&content, style);
        let mut line = Line::from(checkbox);
        line.spans.extend(processed.spans);
        line
    }

    /// Render a line with inline markdown
    fn render_inline_line(&self, line: &str) -> Line<'static> {
        self.render_inline_span(line, Style::default())
    }

    /// Render inline markdown within a span
    fn render_inline_span(&self, text: &str, base_style: Style) -> Line<'static> {
        let elements = self.parse_inline_elements(text);

        // Build composite spans with proper styling for each element
        let mut spans = Vec::new();
        for element in &elements {
            match element {
                InlineElement::Code(code_content) => {
                    // Code gets a distinct style with a subtle background
                    let code_style = Style::default()
                        .fg(Color::Rgb(251, 191, 36))  // Amber/yellow for code
                        .bg(Color::Rgb(40, 40, 45));
                    spans.push(Span::styled(format!("`{}`", code_content), code_style));
                }
                InlineElement::Bold(content) => {
                    let bold_style = base_style.add_modifier(Modifier::BOLD);
                    spans.push(Span::styled(content.clone(), bold_style));
                }
                InlineElement::Italic(content) => {
                    let italic_style = base_style.add_modifier(Modifier::ITALIC);
                    spans.push(Span::styled(content.clone(), italic_style));
                }
                InlineElement::Strikethrough(content) => {
                    let strike_style = base_style.add_modifier(Modifier::CROSSED_OUT);
                    spans.push(Span::styled(content.clone(), strike_style));
                }
                InlineElement::Link { text, .. } => {
                    let link_style = base_style.fg(Color::Rgb(147, 197, 253)).add_modifier(Modifier::UNDERLINED);
                    spans.push(Span::styled(text.clone(), link_style));
                }
                InlineElement::Text(s) => {
                    spans.push(Span::styled(s.clone(), base_style));
                }
            }
        }
        
        Line::from(spans)
    }

    /// Parse inline markdown elements from text
    fn parse_inline_elements(&self, text: &str) -> Vec<InlineElement> {
        let mut elements = Vec::new();
        let mut remaining = text.to_string();

        while !remaining.is_empty() {
            // Find next special pattern
            let bold_pos = self.find_pattern(&remaining, "**");
            let bold_alt_pos = self.find_pattern(&remaining, "__");
            let italic_pos = self.find_pattern(&remaining, "*");
            let italic_alt_pos = self.find_pattern(&remaining, "_");
            let strike_pos = self.find_pattern(&remaining, "~~");
            let code_pos = self.find_pattern(&remaining, "`");
            let link_pos = self.find_pattern(&remaining, "[");

            let positions = [
                (bold_pos, "**"),
                (bold_alt_pos, "__"),
                (italic_pos, "*"),
                (italic_alt_pos, "_"),
                (strike_pos, "~~"),
                (code_pos, "`"),
                (link_pos, "["),
            ];

            // Find the earliest valid pattern
            let mut earliest: Option<(usize, &str)> = None;
            for (pos, pattern) in positions {
                if let Some(p) = pos
                    && earliest.is_none_or(|(e, _)| p < e)
                {
                    // Make sure this isn't part of a longer pattern
                    // e.g., * should not match inside **
                    let is_valid = match pattern {
                        "*" => !remaining.starts_with("**"),
                        "_" => !remaining.starts_with("__"),
                        _ => true,
                    };
                    if is_valid {
                        earliest = Some((p, pattern));
                    }
                }
            }

            if let Some((pos, pattern)) = earliest {
                // Add text before the pattern
                if pos > 0 {
                    elements.push(InlineElement::Text(remaining[..pos].to_string()));
                }

                remaining = remaining[pos..].to_string();

                // Process the pattern
                match pattern {
                    "**" | "__" => {
                        remaining = remaining[pattern.len()..].to_string();
                        if let Some(end) = self.find_matching_delimiter(&remaining, pattern) {
                            let content = &remaining[..end];
                            elements.push(InlineElement::Bold(content.to_string()));
                            remaining = remaining[end + pattern.len()..].to_string();
                        } else {
                            elements.push(InlineElement::Text(pattern.to_string()));
                        }
                    }
                    "*" | "_" => {
                        remaining = remaining[pattern.len()..].to_string();
                        if let Some(end) = self.find_matching_delimiter(&remaining, pattern) {
                            let content = &remaining[..end];
                            elements.push(InlineElement::Italic(content.to_string()));
                            remaining = remaining[end + pattern.len()..].to_string();
                        } else {
                            elements.push(InlineElement::Text(pattern.to_string()));
                        }
                    }
                    "~~" => {
                        remaining = remaining[pattern.len()..].to_string();
                        if let Some(end) = self.find_matching_delimiter(&remaining, "~~") {
                            let content = &remaining[..end];
                            elements.push(InlineElement::Strikethrough(content.to_string()));
                            remaining = remaining[end + 2..].to_string();
                        } else {
                            elements.push(InlineElement::Text("~~".to_string()));
                        }
                    }
                    "`" => {
                        remaining = remaining[pattern.len()..].to_string();
                        if let Some(end) = remaining.find('`') {
                            let content = &remaining[..end];
                            elements.push(InlineElement::Code(content.to_string()));
                            remaining = remaining[end + 1..].to_string();
                        } else {
                            elements.push(InlineElement::Text("`".to_string()));
                        }
                    }
                    "[" => {
                        remaining = remaining[1..].to_string();
                        if let Some(text_end) = remaining.find(']') {
                            let link_text = remaining[..text_end].to_string(); // Clone to avoid borrow issues
                            remaining = remaining[text_end + 1..].to_string();
                            if remaining.starts_with("(") {
                                remaining = remaining[1..].to_string();
                                if let Some(url_end) = remaining.find(')') {
                                    let _url = remaining[..url_end].to_string(); // Clone to avoid borrow issues
                                    elements.push(InlineElement::Link {
                                        text: link_text.clone(),
                                        _url,
                                    });
                                    remaining = remaining[url_end + 1..].to_string();
                                    continue;
                                }
                            }
                            elements.push(InlineElement::Text(format!("[{}", link_text)));
                        } else {
                            elements.push(InlineElement::Text("[".to_string()));
                        }
                    }
                    _ => {
                        elements.push(InlineElement::Text(pattern.to_string()));
                        remaining = remaining[pattern.len()..].to_string();
                    }
                }
            } else {
                // No more patterns
                elements.push(InlineElement::Text(remaining.clone()));
                break;
            }
        }

        elements
    }

    /// Find a pattern in text, returning its position
    fn find_pattern(&self, text: &str, pattern: &str) -> Option<usize> {
        text.find(pattern)
    }

    /// Find the closing delimiter for bold/italic/strikethrough
    /// Handles cases where the delimiter must be followed by whitespace/punctuation
    fn find_matching_delimiter(&self, text: &str, delimiter: &str) -> Option<usize> {
        let mut pos = 0;
        let chars: Vec<char> = text.chars().collect();
        let delim_len = delimiter.len();

        while pos < chars.len() {
            // Find potential delimiter start
            let slice: String = chars[pos..].iter().collect();
            if !slice.starts_with(delimiter) {
                pos += 1;
                continue;
            }

            // Check if this is a valid closing delimiter
            // A closing delimiter should be followed by whitespace, punctuation, or end of string
            let after_pos = pos + delim_len;
            let is_valid_close = if after_pos >= chars.len() {
                true
            } else {
                let next_char = chars[after_pos];
                next_char.is_whitespace() || next_char.is_ascii_punctuation()
            };

            if is_valid_close {
                return Some(pos);
            }

            pos += 1;
        }

        None
    }

    /// Convert inline elements to plain text (for simple rendering)
    fn elements_to_plain_text(&self, elements: &[InlineElement]) -> String {
        let mut result = String::new();
        for element in elements {
            match element {
                InlineElement::Text(s) => result.push_str(s),
                InlineElement::Bold(s)
                | InlineElement::Italic(s)
                | InlineElement::Strikethrough(s) => {
                    result.push_str(s);
                }
                InlineElement::Code(s) => result.push_str(s),
                InlineElement::Link { text, .. } => result.push_str(text),
            }
        }
        result
    }

    /// Highlight code using syntect
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

/// Align text within a given width according to the specified alignment
fn align_text(text: &str, width: usize, alignment: TableAlignment) -> String {
    let text_width = visible_width(text);

    if text_width >= width {
        return text.to_string();
    }

    let padding = width - text_width;

    match alignment {
        TableAlignment::Left => format!("{}{}", text, " ".repeat(padding)),
        TableAlignment::Right => format!("{}{}", " ".repeat(padding), text),
        TableAlignment::Center => {
            let left_pad = padding / 2;
            let right_pad = padding - left_pad;
            format!("{}{}{}", " ".repeat(left_pad), text, " ".repeat(right_pad))
        }
        TableAlignment::Unspecified => format!("{}{}", text, " ".repeat(padding)),
    }
}
