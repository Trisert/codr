//! Status widget for toasts and progress indicators
//!
//! Displays temporary messages and progress indicators
//! to the user.

use ratatui::{buffer::Buffer, layout::Rect, style::Style, widgets::Widget};
use crate::tui::theme::Theme;

/// Message level for toasts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// Toast message (temporary notification)
#[derive(Debug, Clone)]
pub struct ToastMessage {
    /// Message content
    pub text: String,
    /// Message level
    pub level: MessageLevel,
    /// Timestamp when shown
    pub timestamp: std::time::Instant,
    /// Auto-dismiss time (None = manual dismiss)
    pub auto_dismiss: Option<std::time::Duration>,
}

impl ToastMessage {
    /// Create new toast message
    pub fn new(text: String, level: MessageLevel) -> Self {
        let auto_dismiss = match level {
            MessageLevel::Info => Some(std::time::Duration::from_secs(3)),
            MessageLevel::Success => Some(std::time::Duration::from_secs(2)),
            MessageLevel::Warning => Some(std::time::Duration::from_secs(4)),
            MessageLevel::Error => Some(std::time::Duration::from_secs(5)),
        };

        Self {
            text,
            level,
            timestamp: std::time::Instant::now(),
            auto_dismiss,
        }
    }

    /// Check if toast should be dismissed
    pub fn should_dismiss(&self) -> bool {
        if let Some(duration) = self.auto_dismiss {
            self.timestamp.elapsed() >= duration
        } else {
            false
        }
    }

    /// Get color for message level
    pub fn color(&self, theme: &Theme) -> ratatui::style::Color {
        match self.level {
            MessageLevel::Info => theme.info,
            MessageLevel::Success => theme.success,
            MessageLevel::Warning => theme.warning,
            MessageLevel::Error => theme.error,
        }
    }
}

/// Status widget showing toasts and progress
pub struct StatusWidget<'a> {
    /// Theme for styling
    theme: &'a Theme,
    /// Toast messages to display
    toasts: &'a [ToastMessage],
    /// Progress indicator text (optional)
    progress: Option<String>,
    /// Show spinner
    show_spinner: bool,
}

impl<'a> StatusWidget<'a> {
    /// Create new status widget
    pub fn new(theme: &'a Theme, toasts: &'a [ToastMessage]) -> Self {
        Self {
            theme,
            toasts,
            progress: None,
            show_spinner: false,
        }
    }

    /// Set progress text
    pub fn progress(mut self, progress: Option<&str>) -> Self {
        self.progress = progress.map(|s| s.to_string());
        self
    }

    /// Set spinner visibility
    pub fn spinner(mut self, show_spinner: bool) -> Self {
        self.show_spinner = show_spinner;
        self
    }
}

impl<'a> Widget for StatusWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let active_toasts: Vec<_> = self
            .toasts
            .iter()
            .filter(|t| !t.should_dismiss())
            .collect();

        if active_toasts.is_empty() && self.progress.is_none() && !self.show_spinner {
            return;
        }

        // Calculate vertical space needed
        let toast_count = active_toasts.iter().rev().take(3).count();
        let has_progress = self.progress.is_some() || self.show_spinner;
        let progress_height = if has_progress { 1 } else { 0 };
        let total_height = toast_count + progress_height;

        // Start rendering from bottom
        let mut y = area.bottom() - total_height as u16;

        // Render toasts (displayed above progress)
        for toast in active_toasts.iter().rev().take(3) {
            if y >= area.bottom() {
                break;
            }

            let text_style = Style::default()
                .fg(toast.color(self.theme));

            // Simple inline toast: icon + text
            let icon = match toast.level {
                MessageLevel::Info => "→ ",
                MessageLevel::Success => "✓ ",
                MessageLevel::Warning => "⚠ ",
                MessageLevel::Error => "✗ ",
            };

            buf.set_string(area.x + 2, y, icon, text_style);
            buf.set_string(area.x + 4, y, &toast.text, text_style);
            y += 1;
        }

        // Progress/spinner — minimal inline display at bottom
        let progress_y = area.bottom() - 1;
        if let Some(ref progress) = self.progress {
            let style = Style::default().fg(self.theme.dimmed);
            self.render_spinner_inline(area.x + 2, progress_y, buf, style);
            buf.set_string(area.x + 4, progress_y, progress, style);
        } else if self.show_spinner {
            let style = Style::default().fg(self.theme.dimmed);
            self.render_spinner_inline(area.x + 2, progress_y, buf, style);
            buf.set_string(area.x + 4, progress_y, "working...", style);
        }
    }
}

impl<'a> StatusWidget<'a> {
    fn render_spinner_inline(&self, x: u16, y: u16, buf: &mut Buffer, style: Style) {
        let spinner = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
        let frame = spinner.chars().nth(
            (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() / 100) as usize % spinner.len()
        );

        if let Some(ch) = frame {
            buf.set_string(x, y, format!("{}", ch), style);
        }
    }
}
