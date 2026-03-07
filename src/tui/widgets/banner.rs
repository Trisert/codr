//! Banner widget for TUI
//!
//! Displays logo, model info, and status at the top of screen.
//! Codex-style with enhanced status information.

use crate::tui::theme::Theme;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

/// Banner widget showing logo and status
pub struct BannerWidget<'a> {
    /// Theme for styling
    theme: &'a Theme,
    /// Model name to display
    model_name: &'a str,
    /// Current role (mode)
    role: &'a str,
    /// Token usage
    tokens: u32,
    /// Cost in currency
    cost: f64,
    /// Current working directory
    cwd: Option<&'a str>,
    /// Agent status (running/idle)
    agent_status: AgentStatus,
    /// Connection status
    connected: bool,
}

/// Agent status indicator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Running,
    Streaming,
    Error,
}

impl<'a> BannerWidget<'a> {
    /// Create new banner widget
    pub fn new(theme: &'a Theme, model_name: &'a str, role: &'a str) -> Self {
        Self {
            theme,
            model_name,
            role,
            tokens: 0,
            cost: 0.0,
            cwd: None,
            agent_status: AgentStatus::Idle,
            connected: true,
        }
    }

    /// Set token usage
    pub fn tokens(mut self, tokens: u32) -> Self {
        self.tokens = tokens;
        self
    }

    /// Set cost
    pub fn cost(mut self, cost: f64) -> Self {
        self.cost = cost;
        self
    }

    /// Set current working directory
    pub fn cwd(mut self, cwd: Option<&'a str>) -> Self {
        self.cwd = cwd;
        self
    }

    /// Set agent status
    pub fn agent_status(mut self, status: AgentStatus) -> Self {
        self.agent_status = status;
        self
    }

    /// Set connection status
    pub fn connected(mut self, connected: bool) -> Self {
        self.connected = connected;
        self
    }
}

impl<'a> Widget for BannerWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Codex-style separator at bottom
        let separator = "─".repeat(area.width as usize);
        buf.set_string(
            area.x,
            area.bottom() - 1,
            &separator,
            Style::default().fg(self.theme.border),
        );

        // Banner content - Codex style: two lines
        // Line 1: Model name + role + status indicator
        let y1 = area.y;

        // Model name in primary color (OpenAI green)
        let mut spans = vec![
            Span::styled(
                "codr",
                Style::default()
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default()),
        ];

        // Model name
        spans.push(Span::styled(self.model_name, self.theme.dim_style()));

        // Agent status indicator
        let (status_char, status_color) = match self.agent_status {
            AgentStatus::Idle => ("●", self.theme.dimmed),
            AgentStatus::Running => ("●", self.theme.primary),
            AgentStatus::Streaming => ("●", self.theme.secondary),
            AgentStatus::Error => ("●", self.theme.error),
        };
        spans.push(Span::styled(" ", Style::default()));
        spans.push(Span::styled(status_char, Style::default().fg(status_color)));

        // Connection status
        let conn_indicator = if self.connected { "●" } else { "○" };
        spans.push(Span::styled(" ", Style::default()));
        spans.push(Span::styled(
            conn_indicator,
            Style::default().fg(self.theme.dimmed),
        ));

        // Right side: role in brackets
        let role_style = match self.role {
            "SAFE" => Style::default().fg(self.theme.success),
            "YOLO" => Style::default().fg(self.theme.error),
            "PLAN" => Style::default().fg(self.theme.secondary),
            _ => self.theme.dim_style(),
        };

        let role_spans = vec![
            Span::styled(" ", Style::default()),
            Span::styled("[", self.theme.dim_style()),
            Span::styled(self.role, role_style),
            Span::styled("]", self.theme.dim_style()),
        ];

        // Combine left and right parts
        spans.extend(role_spans);

        let line1 = Line::from(spans);
        buf.set_line(area.x, y1, &line1, area.width);

        // Line 2: CWD + tokens + cost (if available)
        let y2 = y1 + 1;
        let mut line2_spans = Vec::new();

        // Working indicator when agent is active
        if matches!(
            self.agent_status,
            AgentStatus::Running | AgentStatus::Streaming
        ) {
            // Animated working message like Claude Code
            let working_messages = [
                "discombambulating...",
                "contemplating...",
                "working...",
                "processing...",
            ];
            let frame = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                / 3) as usize
                % working_messages.len();
            let working_text = working_messages[frame];

            line2_spans.push(Span::styled(
                working_text,
                Style::default()
                    .fg(self.theme.secondary)
                    .add_modifier(Modifier::ITALIC),
            ));
            line2_spans.push(Span::styled("  ", Style::default()));
        }

        // CWD (truncated if too long)
        if let Some(cwd) = self.cwd {
            let max_cwd_len = area.width as usize / 2;
            let display_cwd = if cwd.len() > max_cwd_len {
                format!("…{}", &cwd[cwd.len().saturating_sub(max_cwd_len - 1)..])
            } else {
                cwd.to_string()
            };
            line2_spans.push(Span::styled(display_cwd, self.theme.dim_style()));
            line2_spans.push(Span::styled("  ", Style::default()));
        }

        // Token usage
        if self.tokens > 0 {
            line2_spans.push(Span::styled(
                format!("{} tokens", self.tokens),
                self.theme.dim_style(),
            ));
            line2_spans.push(Span::styled("  ", Style::default()));
        }

        // Cost (if > 0)
        if self.cost > 0.0 {
            line2_spans.push(Span::styled(
                format!("${:.4}", self.cost),
                self.theme.dim_style(),
            ));
        }

        if !line2_spans.is_empty() {
            let line2 = Line::from(line2_spans);
            buf.set_line(area.x, y2, &line2, area.width);
        }
    }
}
