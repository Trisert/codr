//! Theme system for TUI styling
//!
//! This module defines color palettes and styling rules
//! for consistent visual design across the TUI.

use ratatui::style::{Color, Modifier, Style};

/// Theme color palette and style definitions
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    // Base colors
    pub background: Color,
    pub foreground: Color,
    pub dimmed: Color,

    // Syntax highlighting colors
    pub code_keyword: Color,
    pub code_string: Color,
    pub code_comment: Color,
    pub code_function: Color,
    pub code_number: Color,
    pub code_variable: Color,
    pub code_type: Color,
    pub code_attribute: Color,

    // UI element colors
    pub primary: Color,
    pub secondary: Color,
    pub tertiary: Color,
    pub border: Color,

    // Status colors
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,

    // Message type colors
    pub user_message: Color,
    pub assistant_message: Color,
    pub system_message: Color,
    pub thinking_message: Color,
    pub action_message: Color,
    pub output_message: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    // ============================================================
    // Predefined themes
    // ============================================================

    /// Dark theme (default) — Codex-inspired professional design
    pub const fn dark() -> Self {
        Self {
            background: Color::Reset,
            foreground: Color::Rgb(226, 226, 226),
            dimmed: Color::Rgb(118, 118, 118),

            // Syntax colors — Codex-style highlighting
            code_keyword: Color::Rgb(216, 134, 237),
            code_string: Color::Rgb(86, 182, 194),
            code_comment: Color::Rgb(92, 99, 112),
            code_function: Color::Rgb(121, 192, 255),
            code_number: Color::Rgb(248, 134, 134),
            code_variable: Color::Rgb(226, 226, 226),
            code_type: Color::Rgb(255, 121, 198),
            code_attribute: Color::Rgb(248, 134, 134),

            // UI colors — OpenAI green accent (Codex brand)
            primary: Color::Rgb(35, 209, 139),
            secondary: Color::Rgb(58, 136, 255),
            tertiary: Color::Rgb(255, 121, 198),
            border: Color::Rgb(51, 51, 51),

            // Status colors — Codex-style
            success: Color::Rgb(35, 209, 139),
            warning: Color::Rgb(255, 159, 10),
            error: Color::Rgb(255, 85, 85),
            info: Color::Rgb(88, 166, 255),

            // Message colors
            user_message: Color::Rgb(226, 226, 226),
            assistant_message: Color::Rgb(226, 226, 226),
            system_message: Color::Rgb(92, 99, 112),
            thinking_message: Color::Rgb(92, 99, 112),
            action_message: Color::Rgb(118, 118, 118),
            output_message: Color::Rgb(161, 161, 170),
        }
    }

    /// Dracula theme
    pub const fn dracula() -> Self {
        Self {
            background: Color::Rgb(40, 42, 54),
            foreground: Color::Rgb(248, 248, 242),
            dimmed: Color::Rgb(113, 119, 130),

            code_keyword: Color::Rgb(255, 121, 198),
            code_string: Color::Rgb(241, 250, 140),
            code_comment: Color::Rgb(98, 114, 164),
            code_function: Color::Rgb(139, 233, 253),
            code_number: Color::Rgb(189, 147, 249),
            code_variable: Color::Rgb(255, 184, 108),
            code_type: Color::Rgb(80, 250, 123),
            code_attribute: Color::Rgb(241, 250, 140),

            primary: Color::Rgb(189, 147, 249),
            secondary: Color::Rgb(139, 233, 253),
            tertiary: Color::Rgb(80, 250, 123),
            border: Color::Rgb(68, 71, 90),

            success: Color::Rgb(80, 250, 123),
            warning: Color::Rgb(255, 184, 108),
            error: Color::Rgb(255, 85, 85),
            info: Color::Rgb(139, 233, 253),

            user_message: Color::Rgb(255, 184, 108),
            assistant_message: Color::Rgb(189, 147, 249),
            system_message: Color::Rgb(80, 250, 123),
            thinking_message: Color::Rgb(98, 114, 164),
            action_message: Color::Rgb(255, 121, 198),
            output_message: Color::Rgb(248, 248, 242),
        }
    }

    /// Catppuccin Mocha theme
    pub const fn catppuccin_mocha() -> Self {
        Self {
            background: Color::Rgb(30, 30, 46),
            foreground: Color::Rgb(205, 214, 244),
            dimmed: Color::Rgb(153, 165, 200),

            code_keyword: Color::Rgb(203, 166, 247),
            code_string: Color::Rgb(166, 227, 161),
            code_comment: Color::Rgb(108, 112, 134),
            code_function: Color::Rgb(137, 180, 250),
            code_number: Color::Rgb(249, 226, 175),
            code_variable: Color::Rgb(238, 212, 159),
            code_type: Color::Rgb(250, 179, 135),
            code_attribute: Color::Rgb(245, 224, 220),

            primary: Color::Rgb(203, 166, 247),
            secondary: Color::Rgb(137, 180, 250),
            tertiary: Color::Rgb(166, 227, 161),
            border: Color::Rgb(108, 112, 134),

            success: Color::Rgb(166, 227, 161),
            warning: Color::Rgb(239, 159, 118),
            error: Color::Rgb(243, 139, 168),
            info: Color::Rgb(137, 180, 250),

            user_message: Color::Rgb(239, 159, 118),
            assistant_message: Color::Rgb(203, 166, 247),
            system_message: Color::Rgb(166, 227, 161),
            thinking_message: Color::Rgb(108, 112, 134),
            action_message: Color::Rgb(250, 179, 135),
            output_message: Color::Rgb(205, 214, 244),
        }
    }

    /// Tokyo Night theme
    pub const fn tokyo_night() -> Self {
        Self {
            background: Color::Rgb(26, 27, 38),
            foreground: Color::Rgb(169, 177, 214),
            dimmed: Color::Rgb(92, 99, 112),

            code_keyword: Color::Rgb(122, 162, 247),
            code_string: Color::Rgb(158, 206, 106),
            code_comment: Color::Rgb(92, 99, 112),
            code_function: Color::Rgb(187, 154, 247),
            code_number: Color::Rgb(224, 175, 104),
            code_variable: Color::Rgb(239, 148, 133),
            code_type: Color::Rgb(250, 179, 135),
            code_attribute: Color::Rgb(245, 224, 220),

            primary: Color::Rgb(122, 162, 247),
            secondary: Color::Rgb(192, 202, 245),
            tertiary: Color::Rgb(158, 206, 106),
            border: Color::Rgb(92, 99, 112),

            success: Color::Rgb(158, 206, 106),
            warning: Color::Rgb(224, 175, 104),
            error: Color::Rgb(247, 118, 142),
            info: Color::Rgb(122, 162, 247),

            user_message: Color::Rgb(224, 175, 104),
            assistant_message: Color::Rgb(122, 162, 247),
            system_message: Color::Rgb(158, 206, 106),
            thinking_message: Color::Rgb(92, 99, 112),
            action_message: Color::Rgb(187, 154, 247),
            output_message: Color::Rgb(169, 177, 214),
        }
    }

    // ============================================================
    // Style helpers
    // ============================================================

    /// Get style for message type
    pub fn style_for_message_type(&self, role: &str) -> Style {
        match role {
            "user" => Style::default()
                .fg(self.user_message)
                .add_modifier(Modifier::BOLD),
            "assistant" => Style::default().fg(self.assistant_message),
            "system" => Style::default()
                .fg(self.system_message)
                .add_modifier(Modifier::DIM),
            "thinking" => Style::default()
                .fg(self.thinking_message)
                .add_modifier(Modifier::ITALIC),
            _ => Style::default().fg(self.dimmed),
        }
    }

    /// Get style for status/error level
    pub fn style_for_status(&self, is_error: bool) -> Style {
        if is_error {
            Style::default().fg(self.error).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.success)
        }
    }

    /// Get border style
    pub fn border_style(&self) -> Style {
        Style::default().fg(self.border)
    }

    /// Get primary button style
    pub fn button_style(&self, is_hovered: bool) -> Style {
        let mut style = Style::default().fg(self.background).bg(self.primary);
        if is_hovered {
            style = style.add_modifier(Modifier::BOLD);
        }
        style
    }

    /// Get input box style
    pub fn input_style(&self) -> Style {
        Style::default().fg(self.foreground).bg(self.border)
    }

    /// Get cursor style
    pub fn cursor_style(&self) -> Style {
        Style::default()
            .fg(self.background)
            .bg(self.primary)
            .add_modifier(Modifier::BOLD)
    }

    /// Get banner/title style
    pub fn banner_style(&self) -> Style {
        Style::default()
            .fg(self.primary)
            .add_modifier(Modifier::BOLD)
    }

    /// Get dimmed text style
    pub fn dim_style(&self) -> Style {
        Style::default().fg(self.dimmed).add_modifier(Modifier::DIM)
    }

    /// Get action message style
    pub fn action_style(&self) -> Style {
        Style::default().fg(self.action_message)
    }

    /// Get output message style
    pub fn output_style(&self) -> Style {
        Style::default()
            .fg(self.output_message)
            .add_modifier(Modifier::DIM)
    }

    /// Get thinking style
    pub fn thinking_style(&self) -> Style {
        Style::default()
            .fg(self.thinking_message)
            .add_modifier(Modifier::ITALIC)
    }

    /// Get code block border style
    pub fn code_border_style(&self) -> Style {
        Style::default().fg(self.border).add_modifier(Modifier::DIM)
    }

    /// Get selection style
    pub fn selection_style(&self) -> Style {
        Style::default()
            .bg(self.secondary)
            .fg(self.background)
            .add_modifier(Modifier::BOLD)
    }

    /// Get highlight style
    pub fn highlight_style(&self) -> Style {
        Style::default()
            .bg(self.tertiary)
            .fg(self.background)
            .add_modifier(Modifier::BOLD)
    }
}
