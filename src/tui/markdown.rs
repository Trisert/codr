//! Markdown rendering for TUI
//!
//! Based on codex's markdown implementation using pulldown_cmark.

use pulldown_cmark::{CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::Line,
    text::Span,
    text::Text,
};
use unicode_width::UnicodeWidthStr;

/// Styles for markdown elements
#[derive(Clone, Debug)]
struct MarkdownStyles {
    h1: Style,
    h2: Style,
    h3: Style,
    h4: Style,
    h5: Style,
    h6: Style,
    code: Style,
    emphasis: Style,
    strong: Style,
    strikethrough: Style,
    ordered_list_marker: Style,
    unordered_list_marker: Style,
    blockquote: Style,
}

impl Default for MarkdownStyles {
    fn default() -> Self {
        Self {
            h1: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
            h2: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            h3: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::ITALIC),
            h4: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::ITALIC),
            h5: Style::default().add_modifier(Modifier::ITALIC),
            h6: Style::default().add_modifier(Modifier::ITALIC),
            code: Style::default().fg(Color::Rgb(180, 180, 180)),
            emphasis: Style::default().add_modifier(Modifier::ITALIC),
            strong: Style::default().add_modifier(Modifier::BOLD),
            strikethrough: Style::default().add_modifier(Modifier::CROSSED_OUT),
            ordered_list_marker: Style::default().fg(Color::LightBlue),
            unordered_list_marker: Style::default(),
            blockquote: Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::ITALIC),
        }
    }
}

/// Render markdown to ratatui Text
pub fn render_markdown(input: &str, width: usize) -> Text<'static> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(input, options);

    let mut renderer = MarkdownRenderer::new(parser, width);
    renderer.run();
    renderer.text
}

/// Markdown renderer
struct MarkdownRenderer<'a, I>
where
    I: Iterator<Item = Event<'a>>,
{
    iter: std::iter::Peekable<I>,
    text: Text<'static>,
    styles: MarkdownStyles,
    inline_styles: Vec<Style>,
    indent_stack: Vec<IndentContext>,
    list_indices: Vec<Option<u64>>,
    needs_newline: bool,
    pending_marker_line: bool,
    in_paragraph: bool,
    in_code_block: bool,
    code_block_lang: Option<String>,
    code_block_buffer: String,
    wrap_width: usize,
    current_line_content: Option<Line<'static>>,
    current_indent: Vec<Span<'static>>,
    current_line_style: Style,
}

#[derive(Clone, Debug)]
struct IndentContext {
    prefix: Vec<Span<'static>>,
    marker: Option<Vec<Span<'static>>>,
    is_list: bool,
}

impl IndentContext {
    fn new(prefix: Vec<Span<'static>>, marker: Option<Vec<Span<'static>>>, is_list: bool) -> Self {
        Self {
            prefix,
            marker,
            is_list,
        }
    }
}

impl<'a, I> MarkdownRenderer<'a, I>
where
    I: Iterator<Item = Event<'a>>,
{
    fn new(iter: I, wrap_width: usize) -> Self {
        Self {
            iter: iter.peekable(),
            text: Text::default(),
            styles: MarkdownStyles::default(),
            inline_styles: Vec::new(),
            indent_stack: Vec::new(),
            list_indices: Vec::new(),
            needs_newline: false,
            pending_marker_line: false,
            in_paragraph: false,
            in_code_block: false,
            code_block_lang: None,
            code_block_buffer: String::new(),
            wrap_width,
            current_line_content: None,
            current_indent: Vec::new(),
            current_line_style: Style::default(),
        }
    }

    fn run(&mut self) {
        while let Some(ev) = self.iter.next() {
            self.handle_event(ev);
        }
        self.flush_current_line();
    }

    fn handle_event(&mut self, event: Event<'a>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.text(text),
            Event::Code(code) => self.code(code),
            Event::SoftBreak => self.soft_break(),
            Event::HardBreak => self.hard_break(),
            Event::Rule => self.rule(),
            Event::InlineHtml(html) => self.inline_html(html),
            Event::Html(html) => self.html(html),
            Event::FootnoteReference(_) => {}
            Event::TaskListMarker(_) => {}
            Event::InlineMath(_) => {} // Skip math rendering
            Event::DisplayMath(_) => {} // Skip math rendering
        }
    }

    fn start_tag(&mut self, tag: Tag<'a>) {
        match tag {
            Tag::Paragraph => self.start_paragraph(),
            Tag::Heading { level, .. } => self.start_heading(level),
            Tag::BlockQuote(_) => self.start_blockquote(),
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(lang) => Some(lang.to_string()),
                    CodeBlockKind::Indented => None,
                };
                self.start_codeblock(lang)
            }
            Tag::List(start) => self.start_list(start),
            Tag::Item => self.start_item(),
            Tag::Emphasis => self.push_inline_style(self.styles.emphasis),
            Tag::Strong => self.push_inline_style(self.styles.strong),
            Tag::Strikethrough => self.push_inline_style(self.styles.strikethrough),
            Tag::Link { .. } => {} // Skip link rendering for simplicity
            Tag::Image { .. } => {} // Skip image rendering
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.end_paragraph(),
            TagEnd::Heading(_) => self.end_heading(),
            TagEnd::BlockQuote(_) => self.end_blockquote(),
            TagEnd::CodeBlock => self.end_codeblock(),
            TagEnd::List(_) => self.end_list(),
            TagEnd::Item => {
                self.indent_stack.pop();
                self.pending_marker_line = false;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.pop_inline_style();
            }
            _ => {}
        }
    }

    fn start_paragraph(&mut self) {
        if self.needs_newline {
            self.push_blank_line();
        }
        self.push_line(Line::default());
        self.needs_newline = false;
        self.in_paragraph = true;
    }

    fn end_paragraph(&mut self) {
        self.needs_newline = true;
        self.in_paragraph = false;
        self.pending_marker_line = false;
    }

    fn start_heading(&mut self, level: HeadingLevel) {
        if self.needs_newline {
            self.push_line(Line::default());
            self.needs_newline = false;
        }
        let heading_style = match level {
            HeadingLevel::H1 => self.styles.h1,
            HeadingLevel::H2 => self.styles.h2,
            HeadingLevel::H3 => self.styles.h3,
            HeadingLevel::H4 => self.styles.h4,
            HeadingLevel::H5 => self.styles.h5,
            HeadingLevel::H6 => self.styles.h6,
        };
        let content = format!("{} ", "#".repeat(level as usize));
        self.push_line(Line::from(vec![Span::styled(content, heading_style)]));
        self.push_inline_style(heading_style);
        self.needs_newline = false;
    }

    fn end_heading(&mut self) {
        self.needs_newline = true;
        self.pop_inline_style();
    }

    fn start_blockquote(&mut self) {
        if self.needs_newline {
            self.push_blank_line();
            self.needs_newline = false;
        }
        self.indent_stack
            .push(IndentContext::new(vec![Span::from("> ")], None, false));
    }

    fn end_blockquote(&mut self) {
        self.indent_stack.pop();
        self.needs_newline = true;
    }

    fn start_codeblock(&mut self, lang: Option<String>) {
        self.flush_current_line();
        if !self.text.lines.is_empty() {
            self.push_blank_line();
        }
        self.in_code_block = true;

        // Extract language token
        let lang = lang
            .as_deref()
            .and_then(|s| s.split([',', ' ', '\t']).next())
            .filter(|s| !s.is_empty())
            .map(std::string::ToString::to_string);
        self.code_block_lang = lang;
        self.code_block_buffer.clear();
        self.needs_newline = true;
    }

    fn end_codeblock(&mut self) {
        // Add buffered code as plain text
        if let Some(_lang) = self.code_block_lang.take() {
            let code: String = std::mem::take(&mut self.code_block_buffer);
            for line in code.lines() {
                self.push_line(Line::default());
                self.push_span(Span::styled(line.to_string(), self.styles.code));
            }
        } else {
            let code: String = std::mem::take(&mut self.code_block_buffer);
            for line in code.lines() {
                self.push_line(Line::default());
                let style = self.inline_styles.last().copied().unwrap_or_default();
                self.push_span(Span::styled(line.to_string(), style));
            }
        }

        self.needs_newline = true;
        self.in_code_block = false;
    }

    fn start_list(&mut self, index: Option<u64>) {
        if self.list_indices.is_empty() && self.needs_newline {
            self.push_line(Line::default());
        }
        self.list_indices.push(index);
    }

    fn end_list(&mut self) {
        self.list_indices.pop();
        self.needs_newline = true;
    }

    fn start_item(&mut self) {
        self.pending_marker_line = true;
        let depth = self.list_indices.len();
        let is_ordered = self
            .list_indices
            .last()
            .map(Option::is_some)
            .unwrap_or(false);
        let width = depth * 4 - 3;
        let marker = if let Some(last_index) = self.list_indices.last_mut() {
            match last_index {
                None => Some(vec![Span::styled(
                    " ".repeat(width.saturating_sub(1)) + "- ",
                    self.styles.unordered_list_marker,
                )]),
                Some(index) => {
                    *index += 1;
                    Some(vec![Span::styled(
                        format!("{:width$}. ", index.saturating_sub(1)),
                        self.styles.ordered_list_marker,
                    )])
                }
            }
        } else {
            None
        };
        let indent_prefix = if depth == 0 {
            Vec::new()
        } else {
            let indent_len = if is_ordered { width + 2 } else { width + 1 };
            vec![Span::from(" ".repeat(indent_len))]
        };
        self.indent_stack
            .push(IndentContext::new(indent_prefix, marker, true));
        self.needs_newline = false;
    }

    fn text(&mut self, text: CowStr<'a>) {
        if self.pending_marker_line {
            self.push_line(Line::default());
        }
        self.pending_marker_line = false;

        // Accumulate code block text
        if self.in_code_block && self.code_block_lang.is_some() {
            self.code_block_buffer.push_str(&text);
            return;
        }

        // Handle text line by line
        for (i, line) in text.lines().enumerate() {
            if self.needs_newline {
                self.push_line(Line::default());
                self.needs_newline = false;
            }
            if i > 0 {
                self.push_line(Line::default());
            }
            let span = Span::styled(
                line.to_string(),
                self.inline_styles.last().copied().unwrap_or_default(),
            );
            self.push_span(span);
        }
        self.needs_newline = false;
    }

    fn code(&mut self, code: CowStr<'a>) {
        if self.pending_marker_line {
            self.push_line(Line::default());
            self.pending_marker_line = false;
        }
        let span = Span::styled(code.into_string(), self.styles.code);
        self.push_span(span);
    }

    fn soft_break(&mut self) {
        self.push_line(Line::default());
    }

    fn hard_break(&mut self) {
        self.push_line(Line::default());
    }

    fn rule(&mut self) {
        self.flush_current_line();
        if !self.text.lines.is_empty() {
            self.push_blank_line();
        }
        self.push_line(Line::from("———"));
        self.needs_newline = true;
    }

    fn inline_html(&mut self, html: CowStr<'a>) {
        // Skip inline HTML
        let _ = html;
    }

    fn html(&mut self, html: CowStr<'a>) {
        // Skip HTML blocks
        let _ = html;
    }

    fn push_inline_style(&mut self, style: Style) {
        let current = self.inline_styles.last().copied().unwrap_or_default();
        let merged = current.patch(style);
        self.inline_styles.push(merged);
    }

    fn pop_inline_style(&mut self) {
        self.inline_styles.pop();
    }

    fn flush_current_line(&mut self) {
        if let Some(line) = self.current_line_content.take() {
            let style = self.current_line_style;

            // Wrap text if not in code block
            if !self.in_code_block {
                let wrapped = self.wrap_line(&line, style);
                for wrapped_line in wrapped {
                    self.text.lines.push(wrapped_line);
                }
            } else {
                let mut spans = self.current_indent.clone();
                spans.extend(line.spans);
                self.text.lines.push(Line::from_iter(spans).style(style));
            }
            self.current_indent.clear();
        }
    }

    fn wrap_line(&self, line: &Line, style: Style) -> Vec<Line<'static>> {
        let mut result = Vec::new();
        let mut current_line = Line::default();
        let mut current_width = 0;

        // Add indent
        for span in &self.current_indent {
            current_width += span.content.width();
            current_line.spans.push(span.clone());
        }

        for span in &line.spans {
            let content = &span.content;
            let span_width = content.width();
            let span_style = style.patch(span.style);

            // Check if we need to wrap
            if current_width + span_width > self.wrap_width && current_width > self.current_indent_width() {
                // Start new line
                result.push(std::mem::take(&mut current_line));
                current_width = 0;

                // Add indent to new line
                for span in &self.current_indent {
                    current_width += span.content.width();
                    current_line.spans.push(span.clone());
                }
            }

            // If the span itself is too long, split it
            if current_width + span_width > self.wrap_width {
                let content_str = content.to_string();
                let remaining = self.wrap_width - current_width;

                if remaining > 0 {
                    let first_part: String = content_str.chars().take(remaining).collect();
                    current_line.spans.push(Span::styled(first_part, span_style));
                }

                // Start new line and continue
                result.push(std::mem::take(&mut current_line));
                current_width = 0;

                // Add indent
                for span in &self.current_indent {
                    current_width += span.content.width();
                    current_line.spans.push(span.clone());
                }

                // Add remaining content
                let remaining_content: String = content_str.chars().skip(remaining).collect();
                if !remaining_content.is_empty() {
                    current_line
                        .spans
                        .push(Span::styled(remaining_content.clone(), span_style));
                    current_width += remaining_content.width();
                }
            } else {
                current_line.spans.push(Span::styled(content.to_string(), span_style));
                current_width += span_width;
            }
        }

        if !current_line.spans.is_empty() {
            result.push(current_line);
        }

        if result.is_empty() {
            result.push(Line::default());
        }

        result
    }

    fn current_indent_width(&self) -> usize {
        self.current_indent.iter().map(|s| s.content.width()).sum()
    }

    fn push_line(&mut self, line: Line<'static>) {
        self.flush_current_line();

        let blockquote_active = self
            .indent_stack
            .iter()
            .any(|ctx| ctx.prefix.iter().any(|s| s.content.contains('>')));
        let style = if blockquote_active {
            self.styles.blockquote
        } else {
            line.style
        };

        self.current_indent = self.prefix_spans(self.pending_marker_line);
        self.current_line_style = style;
        self.current_line_content = Some(line);
        self.pending_marker_line = false;
    }

    fn push_span(&mut self, span: Span<'static>) {
        if let Some(line) = self.current_line_content.as_mut() {
            line.spans.push(span);
        } else {
            self.push_line(Line::from(vec![span]));
        }
    }

    fn push_blank_line(&mut self) {
        self.flush_current_line();
        self.text.lines.push(Line::default());
    }

    fn prefix_spans(&self, pending_marker_line: bool) -> Vec<Span<'static>> {
        let mut prefix: Vec<Span<'static>> = Vec::new();
        let last_marker_index = if pending_marker_line {
            self.indent_stack
                .iter()
                .enumerate()
                .rev()
                .find_map(|(i, ctx)| if ctx.marker.is_some() { Some(i) } else { None })
        } else {
            None
        };
        let last_list_index = self.indent_stack.iter().rposition(|ctx| ctx.is_list);

        for (i, ctx) in self.indent_stack.iter().enumerate() {
            if pending_marker_line {
                if Some(i) == last_marker_index && let Some(marker) = &ctx.marker {
                    prefix.extend(marker.iter().cloned());
                    continue;
                }
                if ctx.is_list && last_marker_index.is_some_and(|idx| idx > i) {
                    continue;
                }
            } else if ctx.is_list && Some(i) != last_list_index {
                continue;
            }
            prefix.extend(ctx.prefix.iter().cloned());
        }

        prefix
    }
}
