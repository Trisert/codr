//! Conversation widget for displaying chat messages
//!
//! Renders message history with markdown support,
//! code blocks, and thinking sections.
//! Codex-style with cleaner layout and proper spacing.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use crate::tui::markdown::render_markdown;
use crate::tui::theme::Theme;
use crate::model::Message;

/// Padding for conversation widget
const PADDING: u16 = 2;

/// Conversation widget displaying message history
pub struct ConversationWidget<'a> {
    /// Messages to display
    messages: &'a [Message],
    /// Theme for styling
    theme: &'a Theme,
    /// Current scroll offset
    scroll_offset: usize,
    /// Pending action (for approve/reject workflow)
    pending_action: Option<PendingAction>,
}

/// Pending action for approve/reject workflow
#[derive(Debug, Clone)]
pub struct PendingAction {
    /// Action type (e.g., "bash", "write")
    pub action_type: String,
    /// Action content
    pub content: String,
}

impl<'a> ConversationWidget<'a> {
    /// Create new conversation widget
    pub fn new(messages: &'a [Message], theme: &'a Theme) -> Self {
        Self {
            messages,
            theme,
            scroll_offset: 0,
            pending_action: None,
        }
    }

    /// Set scroll offset
    pub fn scroll_offset(mut self, offset: usize) -> Self {
        self.scroll_offset = offset;
        self
    }

    /// Set pending action
    pub fn pending_action(mut self, action: Option<PendingAction>) -> Self {
        self.pending_action = action;
        self
    }

    /// Scroll up by n lines (hide newest messages)
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
    }

    /// Scroll down by n lines (show more newest messages)
    pub fn scroll_down(&mut self, n: usize, _max_offset: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Scroll to bottom (show all messages)
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Get max scroll offset (max messages to hide)
    pub fn max_scroll_offset(&self) -> usize {
        self.messages.len().saturating_sub(1)
    }
}

impl<'a> Widget for ConversationWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let pad = 2;
        let x = area.x + pad;

        // Chat scroll logic:
        // - scroll_offset = 0: show all messages (auto-scroll to bottom)
        // - scroll_offset > 0: hide that many newest messages (scroll up to see older)
        let visible_count = self.messages.len().saturating_sub(self.scroll_offset);

        // Calculate heights of visible messages in order (oldest to newest)
        // We take from the beginning since scroll_offset controls how many to skip from the end
        let mut message_heights: Vec<u16> = Vec::new();
        let mut total_height: u16 = 0;
        for message in self.messages.iter().take(visible_count) {
            let (spacing, add_bottom) = match &*message.role {
                "user" => (1, false),
                "assistant" => (1, false),
                "action" => (0, false),
                "output" => (0, true),
                "thinking" => (0, false),
                _ => (0, false),
            };

            let content_height: u16 = message.content.lines().count() as u16;
            let mut msg_height = spacing as u16 + content_height;
            if add_bottom {
                msg_height += 1;
            }
            message_heights.push(msg_height);
            total_height += msg_height;
        }

        // Determine starting Y position:
        // - If all messages fit in viewport: position so newest message is at bottom
        // - If messages overflow: start from top and let messages flow downward
        let viewport_height = area.bottom() - area.y - 2;
        let start_y = if total_height < viewport_height {
            // All messages fit - anchor at bottom
            area.bottom() - 2 - total_height
        } else {
            // Messages exceed viewport - start from top
            area.y
        };

        // Render messages in order (oldest at top, newest at bottom)
        let mut y = start_y;

        for message in self.messages.iter().take(visible_count) {
            // Don't render below viewport
            if y > area.bottom() - 2 {
                break;
            }

            // Calculate spacing for this message type
            let (spacing, add_bottom) = match &*message.role {
                "user" => (1, false),
                "assistant" => (1, false),
                "action" => (0, false),
                "output" => (0, true),
                "thinking" => (0, false),
                _ => (0, false),
            };

            // Add spacing before thinking messages
            if &*message.role == "thinking" && spacing == 0 {
                y += 1;
            }

            // Add spacing for this message
            y += spacing as u16;

            // Render the message
            y = self.render_message(message, x, y, area, buf, add_bottom);

            // Add bottom spacing for output messages (which have borders)
            if add_bottom {
                y += 1;
            }
        }

        // Render pending action (approve/reject workflow) at bottom
        if let Some(ref action) = self.pending_action {
            // Ensure we have space for the action box
            if y + 5 <= area.bottom() {
                let _ = self.render_pending_action(action, x, y, area, buf);
            }
        }

        // Scroll indicator - show when we've scrolled up (skip > 0)
        if self.scroll_offset > 0 {
            let scroll_text = "↑ more";
            let scroll_style = Style::default().fg(self.theme.dimmed);
            buf.set_string(area.right() - scroll_text.len() as u16, area.y, scroll_text, scroll_style);
        }
    }
}

impl<'a> ConversationWidget<'a> {
    /// Render a single message
    fn render_message(&self, message: &Message, x: u16, mut y: u16, area: Rect, buf: &mut Buffer, add_bottom: bool) -> u16 {
        let max_width = (area.width - 4) as usize; // pad * 2
        let max_width_u16 = area.width - 4; // For calculations that need u16

        match &*message.role {
            "user" => {
                // User message: ❯ prefix, no extra spacing
                let prefix = "❯ ";
                let prefix_style = Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD);
                let content_style = Style::default().fg(self.theme.foreground);

                buf.set_string(x, y, prefix, prefix_style);

                let content_x = x + 2;
                for line in message.content.lines() {
                    if y >= area.bottom() - 2 {
                        break;
                    }

                    let (wrapped_text, _line_count) = self.wrap_text(line, content_x, area, max_width);
                    for wrapped_line in wrapped_text.lines() {
                        if y >= area.bottom() - 2 {
                            break;
                        }
                        buf.set_string(content_x, y, wrapped_line, content_style);
                        y += 1;
                    }
                }
            }

            "assistant" => {
                // Assistant message: render markdown using pulldown-cmark
                let rendered = render_markdown(message.content.as_ref(), max_width);
                let area_y = y;

                for (i, line) in rendered.lines.iter().enumerate() {
                    if y >= area.bottom() - 2 {
                        break;
                    }

                    // Skip empty lines at the start (already handled by spacing)
                    if i == 0 && line.spans.is_empty() {
                        continue;
                    }

                    // Render each line from the markdown
                    let mut x_offset = 0;
                    for span in &line.spans {
                        if x + x_offset + span.content.width() as u16 >= area.right() - 2 {
                            break;
                        }
                        buf.set_span(x + x_offset, y, span, max_width as u16);
                        x_offset += span.content.width() as u16;
                    }
                    y += 1;
                }

                // Ensure we progressed at least one line
                if y == area_y && !message.content.is_empty() {
                    y += 1;
                }
            }

            "action" => {
                // Action message: with ↳ indicator
                let prefix = "↳ ";
                let prefix_style = Style::default().fg(self.theme.dimmed);
                let content_style = Style::default().fg(self.theme.dimmed).add_modifier(Modifier::ITALIC);

                buf.set_string(x, y, prefix, prefix_style);

                // Align content after the prefix
                let content_x = x + 2;
                for line in message.content.lines() {
                    if y >= area.bottom() - 2 {
                        break;
                    }

                    let (wrapped_text, _line_count) = self.wrap_text(line, content_x, area, max_width);
                    for wrapped_line in wrapped_text.lines() {
                        if y >= area.bottom() - 2 {
                            break;
                        }
                        buf.set_string(content_x, y, wrapped_line, content_style);
                        y += 1;
                    }
                }
            }

            "output" => {
                // Output: subtle background with border
                let output_bg = Color::Rgb(25, 25, 30);
                let border_style = Style::default().fg(Color::Rgb(58, 58, 66));
                let content_style = Style::default().fg(Color::Rgb(180, 180, 180)).bg(output_bg);

                // Calculate content lines
                let content_lines: Vec<&str> = message.content.lines().take(4).collect();

                // Indent output for visual separation
                let x = x + 1;

                // Top border
                let top_border = format!("┌─{}┐", "─".repeat(max_width_u16.saturating_sub(2) as usize));
                buf.set_string(x, y, &top_border, border_style);
                y += 1;

                // Content with borders
                for line in content_lines {
                    let line_text = self.truncate_line(line, max_width.saturating_sub(4));
                    let padding = max_width_u16.saturating_sub(4).saturating_sub(line_text.len() as u16) as usize;
                    let border_line = format!("│ {}{}│", line_text, " ".repeat(padding));
                    buf.set_string(x, y, &border_line, content_style);
                    y += 1;
                }

                // Bottom border
                let bottom_border = format!("└─{}┘", "─".repeat(max_width_u16.saturating_sub(2) as usize));
                buf.set_string(x, y, &bottom_border, border_style);
                y += 1;
            }

            "thinking" => {
                // Thinking: dim italic, unobtrusive
                let thinking_style = Style::default()
                    .fg(self.theme.thinking_message)
                    .add_modifier(Modifier::ITALIC)
                    .add_modifier(Modifier::DIM);

                for line in message.content.lines() {
                    if y >= area.bottom() - 2 {
                        break;
                    }

                    let (wrapped_text, _line_count) = self.wrap_text(line, x, area, max_width);
                    for wrapped_line in wrapped_text.lines() {
                        if y >= area.bottom() - 2 {
                            break;
                        }
                        buf.set_string(x, y, wrapped_line, thinking_style);
                        y += 1;
                    }
                }
            }

            _ => {
                // Other messages: dim
                let content_style = Style::default().fg(self.theme.dimmed);

                for line in message.content.lines() {
                    if y >= area.bottom() - 2 {
                        break;
                    }

                    let (wrapped_text, _line_count) = self.wrap_text(line, x, area, max_width);
                    for wrapped_line in wrapped_text.lines() {
                        if y >= area.bottom() - 2 {
                            break;
                        }
                        buf.set_string(x, y, wrapped_line, content_style);
                        y += 1;
                    }
                }
            }
        }

        // Add bottom spacing if requested (for output messages with borders)
        if add_bottom && y < area.bottom() - 2 {
            y += 1;
        }

        y
    }

    /// Render pending action (approve/reject workflow)
    fn render_pending_action(&self, action: &PendingAction, x: u16, y: u16, _area: Rect, buf: &mut Buffer) -> u16 {
        let max_width = (_area.width - PADDING * 2) as usize;

        // Draw action box
        let box_style = Style::default().fg(self.theme.warning).bg(Color::Rgb(40, 35, 30));

        // Border
        let border = format!("┌{}┐", "─".repeat(max_width.saturating_sub(2)));
        buf.set_string(x, y, &border, box_style);

        // Action type
        let action_text = format!(" {} {} ", action.action_type, "requires approval");
        buf.set_string(x + 1, y + 1, &action_text, box_style);

        // Content (truncated)
        let content_preview = self.truncate_line(&action.content, max_width.saturating_sub(4));
        buf.set_string(x + 2, y + 2, &content_preview, Style::default().fg(self.theme.foreground));

        // Buttons
        let approve_text = "[a] Approve";
        let reject_text = "[r] Reject";
        let button_style = Style::default().fg(self.theme.success).add_modifier(Modifier::BOLD);
        let reject_style = Style::default().fg(self.theme.error).add_modifier(Modifier::BOLD);

        buf.set_string(x + 1, y + 3, approve_text, button_style);
        buf.set_string(x + approve_text.len() as u16 + 3, y + 3, reject_text, reject_style);

        // Bottom border
        let bottom_border = format!("└{}┘", "─".repeat(max_width.saturating_sub(2)));
        buf.set_string(x, y + 4, &bottom_border, box_style);

        y + 5
    }

    /// Wrap text to fit width (word-aware using textwrap library)
    fn wrap_text(&self, text: &str, _x: u16, _area: Rect, max_width: usize) -> (String, u16) {
        if text.is_empty() {
            return (String::new(), 1);
        }

        // Use textwrap for proper word wrapping
        let options = textwrap::Options::new(max_width)
            .word_separator(textwrap::WordSeparator::AsciiSpace)
            .break_words(false);

        let wrapped = textwrap::fill(text, options);

        // Count lines
        let line_count = wrapped.lines().count() as u16;

        (wrapped, line_count)
    }

    /// Truncate line to fit width
    fn truncate_line(&self, line: &str, max_width: usize) -> String {
        if line.width() <= max_width {
            line.to_string()
        } else {
            let mut result = String::new();
            let mut current_width = 0;
            for c in line.chars() {
                let char_width = c.width().unwrap_or(1);
                if current_width + char_width > max_width.saturating_sub(1) {
                    result.push('…');
                    break;
                }
                result.push(c);
                current_width += char_width;
            }
            result
        }
    }
}
