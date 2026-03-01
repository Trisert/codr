use crate::error::AgentError;
use crate::logo;
use crate::model::{Message, Model};
use crate::parser::{Action, parse_actions};
use crate::tools::{ToolRegistry, Role};
use crate::tui_components::{
    ApprovalState, ChatMessage, PendingAction, THEME,
    render_message,
};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

// ── Toast Notification System ─────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Toast {
    pub message: String,
    pub timestamp: Instant,
    pub duration: Duration,
}

impl Toast {
    pub fn new(message: String) -> Self {
        Self {
            message,
            timestamp: Instant::now(),
            duration: Duration::from_secs(2),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.timestamp.elapsed() > self.duration
    }
}

// ── Selection State ───────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum SelectionMode {
    None,
    Selecting,
    Selected,
}

#[derive(Clone, Debug)]
pub struct SelectionState {
    pub mode: SelectionMode,
    pub start_line: usize,
    pub end_line: usize,
    pub anchor_line: usize, // For extending selection
}

impl SelectionState {
    pub fn new() -> Self {
        Self {
            mode: SelectionMode::None,
            start_line: 0,
            end_line: 0,
            anchor_line: 0,
        }
    }

    pub fn is_active(&self) -> bool {
        self.mode != SelectionMode::None
    }

    pub fn get_range(&self) -> (usize, usize) {
        let (start, end) = if self.start_line <= self.end_line {
            (self.start_line, self.end_line)
        } else {
            (self.end_line, self.start_line)
        };
        (start, end)
    }

    pub fn contains_line(&self, line: usize) -> bool {
        if !self.is_active() {
            return false;
        }
        let (start, end) = self.get_range();
        line >= start && line <= end
    }
}

// ── Scroll State ─────────────────────────────────────────────────

#[derive(Clone, Debug, Copy)]
pub struct ScrollState {
    pub offset: usize,      // Line index at the top of viewport (0 = top)
    pub total_lines: usize, // Total rendered lines
    pub viewport_height: usize,
    pub auto_scroll: bool, // Auto-scroll to bottom on new content
}

impl ScrollState {
    pub fn new() -> Self {
        Self {
            offset: 0,
            total_lines: 0,
            viewport_height: 0,
            auto_scroll: true,
        }
    }

    pub fn is_at_bottom(&self) -> bool {
        if self.total_lines == 0 || self.viewport_height == 0 {
            return true;
        }
        self.offset + self.viewport_height >= self.total_lines
    }

    pub fn scroll_percentage(&self) -> f32 {
        if self.total_lines == 0
            || self.viewport_height == 0
            || self.viewport_height >= self.total_lines
        {
            return 100.0;
        }
        let max_scroll = self.total_lines.saturating_sub(self.viewport_height);
        if max_scroll == 0 {
            return 100.0;
        }
        ((self.offset as f32) / (max_scroll as f32) * 100.0).min(100.0)
    }

    pub fn scroll_to(&mut self, offset: usize) {
        let max_scroll = self.total_lines.saturating_sub(self.viewport_height);
        self.offset = offset.min(max_scroll);
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let new_offset = if delta >= 0 {
            self.offset.saturating_add(delta as usize)
        } else {
            self.offset.saturating_sub((-delta) as usize)
        };
        self.scroll_to(new_offset);

        // Disable auto-scroll when manually scrolling
        if delta != 0 {
            self.auto_scroll = false;
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.offset = 0;
        self.auto_scroll = false;
    }

    pub fn scroll_to_bottom(&mut self) {
        let viewport = self.viewport_height.max(1); // Avoid division by zero
        if self.total_lines > viewport {
            self.offset = self.total_lines - viewport;
        } else {
            self.offset = 0;
        }
        self.auto_scroll = true;
    }

    pub fn page_up(&mut self) {
        let page_size = self.viewport_height.max(1) / 2;
        self.scroll_by(-(page_size as isize));
    }

    pub fn page_down(&mut self) {
        let page_size = self.viewport_height.max(1) / 2;
        self.scroll_by(page_size as isize);
    }

    pub fn update_total_lines(&mut self, total: usize) {
        self.total_lines = total;
        if self.auto_scroll {
            self.scroll_to_bottom();
        } else {
            // Clamp offset if total lines decreased
            self.scroll_to(self.offset);
        }
    }
}

// For clipboard operations (click-to-copy)
fn copy_to_clipboard(text: &str) -> bool {
    use clipboard::ClipboardProvider;

    // Check if we're on Wayland
    let is_wayland = std::env::var("XDG_SESSION_TYPE")
        .map(|s| s == "wayland")
        .unwrap_or(false);

    // Method 1: On Wayland, prefer wl-copy directly (more reliable)
    #[cfg(target_os = "linux")]
    if is_wayland {
        use std::process::Command;
        if Command::new("wl-copy")
            .arg(text)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|p| p.id() > 0)
            .unwrap_or(false)
        {
            return true;
        }
    }

    // Method 2: Try clipboard crate
    if let Ok(mut ctx) = clipboard::ClipboardContext::new()
        && ctx.set_contents(text.to_string()).is_ok()
    {
        return true;
    } // Fall through to other methods

    // Method 3: Try OSC 52 escape sequence (works in kitty, wezterm, iterm2)
    if let Ok(encoded) = simple_base64_encode(text) {
        let osc52 = format!("\x1b]52;c;{}\x07", encoded);
        if io::stdout().write_all(osc52.as_bytes()).is_ok() {
            let _ = io::stdout().flush();
            // OSC 52 might have worked, but we can't verify
            return true;
        }
    }

    // Method 4: Direct xclip command (most reliable on Linux X11)
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        if Command::new("xclip")
            .arg("-selection")
            .arg("clipboard")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                let mut stdin = child.stdin.as_ref().unwrap();
                stdin.write_all(text.as_bytes())?;
                stdin.flush()?;
                let _ = stdin; // Close stdin to signal EOF
                child.wait()
            })
            .is_ok()
        {
            return true;
        }
    }

    false
}

fn paste_from_clipboard() -> Option<String> {
    use clipboard::ClipboardProvider;

    // Check if we're on Wayland
    let is_wayland = std::env::var("XDG_SESSION_TYPE")
        .map(|s| s == "wayland")
        .unwrap_or(false);

    // Method 1: On Wayland, prefer wl-paste directly
    #[cfg(target_os = "linux")]
    if is_wayland {
        use std::process::Command;
        if let Ok(output) = Command::new("wl-paste").arg("--type=text").output()
            && output.status.success()
        {
            let content = String::from_utf8_lossy(&output.stdout).to_string();
            if !content.is_empty() {
                return Some(content.trim_end().to_string());
            }
        }
    }

    // Method 2: Try clipboard crate
    if let Ok(mut ctx) = clipboard::ClipboardContext::new()
        && let Ok(content) = ctx.get_contents()
    {
        return Some(content.trim_end().to_string());
    }

    // Method 3: Try xclip on Linux
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("xclip")
            .arg("-selection")
            .arg("clipboard")
            .arg("-o")
            .output()
            && output.status.success()
        {
            let content = String::from_utf8_lossy(&output.stdout).to_string();
            if !content.is_empty() {
                return Some(content.trim_end().to_string());
            }
        }
    }

    None
}

// Simple base64 encoding for OSC 52 clipboard
fn simple_base64_encode(input: &str) -> Result<String, String> {
    const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::new();
    let mut buffer = [0u8; 3];

    for chunk in input.as_bytes().chunks(3) {
        buffer[0] = chunk[0];
        buffer[1] = if chunk.len() > 1 { chunk[1] } else { 0 };
        buffer[2] = if chunk.len() > 2 { chunk[2] } else { 0 };

        let triple = (buffer[0] as u32) << 16 | (buffer[1] as u32) << 8 | (buffer[2] as u32);

        result.push(BASE64_CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(BASE64_CHARS[((triple >> 12) & 0x3F) as usize] as char);
        result.push(BASE64_CHARS[((triple >> 6) & 0x3F) as usize] as char);
        result.push(BASE64_CHARS[(triple & 0x3F) as usize] as char);
    }

    // Handle padding
    let remainder = input.len() % 3;
    if remainder == 1 {
        result.push_str("==");
    } else if remainder == 2 {
        result.push('=');
    }

    Ok(result)
}

// ── Messages from background agent task to the UI ────────────

enum AgentUpdate {
    StreamingChunk(Arc<str>),
    StreamingThinkingChunk(Arc<str>),
    ActionMessage(Arc<str>),
    OutputMessage(Arc<String>),
    ErrorMessage(Arc<str>),
    SystemMessage(Arc<str>),
    UsageUpdate { input_tokens: u32, output_tokens: u32, cost: f64 },
    ParallelToolCount(usize),
    Done,
}

// ── Background agent loop (runs in separate task) ───────────────

async fn agent_loop(
    model: Model,
    tool_registry: Arc<ToolRegistry>,
    _tools_description: String,
    mut conversation: Vec<Message>,
    tx: mpsc::UnboundedSender<AgentUpdate>,
    cancel_token: CancellationToken,
    role: Role,
) {
    loop {
        // Check for cancellation
        if cancel_token.is_cancelled() {
            let _ = tx.send(AgentUpdate::SystemMessage("interrupted".into()));
            let _ = tx.send(AgentUpdate::Done);
            return;
        }

        let cancel_token_clone = cancel_token.clone();
        let tx_clone = tx.clone();
        let cancel_token_thinking = cancel_token.clone();
        let tx_thinking = tx.clone();
        let cancel_token_for_query = cancel_token.clone();

        // Use native tool calling if the model supports it
        let lm_output = if model.supports_native_tools() {
            let tools_for_role = tool_registry.get_tools_for_role(role);
            let tools_refs: Vec<&dyn crate::tools::Tool> = tools_for_role;
            model
                .query_streaming_with_tools(
                    &conversation,
                    &tools_refs,
                    move |chunk| {
                        if cancel_token_clone.is_cancelled() {
                            return;
                        }
                        let _ = tx_clone.send(AgentUpdate::StreamingChunk(chunk.into()));
                    },
                    move |thinking| {
                        if cancel_token_thinking.is_cancelled() {
                            return;
                        }
                        let _ = tx_thinking.send(AgentUpdate::StreamingThinkingChunk(thinking.into()));
                    },
                    &cancel_token_for_query,
                )
                .await
        } else {
            model
                .query_streaming(
                    &conversation,
                    move |chunk| {
                        if cancel_token_clone.is_cancelled() {
                            return;
                        }
                        let _ = tx_clone.send(AgentUpdate::StreamingChunk(chunk.into()));
                    },
                    move |thinking| {
                        if cancel_token_thinking.is_cancelled() {
                            return;
                        }
                        let _ = tx_thinking.send(AgentUpdate::StreamingThinkingChunk(thinking.into()));
                    },
                    &cancel_token_for_query,
                )
                .await
        };

        let lm_output = match lm_output {
            Ok(output) => output,
            Err(e) => {
                // Check if error is due to cancellation
                if cancel_token.is_cancelled() || e.to_string().contains("cancelled") {
                    let _ = tx.send(AgentUpdate::SystemMessage("Request cancelled".into()));
                    let _ = tx.send(AgentUpdate::Done);
                    return;
                }
                let _ = tx.send(AgentUpdate::ErrorMessage(format!("Query error: {}", e).into()));
                let _ = tx.send(AgentUpdate::Done);
                return;
            }
        };

        // Check for cancellation after query
        if cancel_token.is_cancelled() {
            let _ = tx.send(AgentUpdate::SystemMessage("interrupted".into()));
            let _ = tx.send(AgentUpdate::Done);
            return;
        }

        // Send usage update if available
        if let Ok(usage) = model.get_usage() {
            let _ = tx.send(AgentUpdate::UsageUpdate {
                input_tokens: usage.prompt_tokens.unwrap_or(0),
                output_tokens: usage.completion_tokens.unwrap_or(0),
                cost: usage.cost_in_currency.unwrap_or(0.0),
            });
        }

        let parsed = match parse_actions(&lm_output) {
            Ok(p) => p,
            Err(AgentError::Terminating(msg)) => {
                let _ = tx.send(AgentUpdate::ErrorMessage(msg.into()));
                let _ = tx.send(AgentUpdate::Done);
                return;
            }
            Err(AgentError::Timeout(msg)) => {
                let _ = tx.send(AgentUpdate::ErrorMessage((&*msg).to_string().into()));
                conversation.push(Message {
                    role: "user".into(),
                    content: Arc::new(msg),
                    images: Vec::new(),
                });
                continue;
            }
        };

        // Send parallel tool count to UI
        let tool_count = parsed.actions.iter()
            .filter(|a| !matches!(a, Action::Response(_)))
            .count();
        let _ = tx.send(AgentUpdate::ParallelToolCount(tool_count));

        // Check for cancellation after parsing
        if cancel_token.is_cancelled() {
            let _ = tx.send(AgentUpdate::SystemMessage("interrupted".into()));
            let _ = tx.send(AgentUpdate::Done);
            return;
        }

        // Process actions with retry logic
        let max_retries = 3;
        let mut retry_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        // First pass: execute all tool/bash actions
        // Second pass: handle response (if any) after all tools complete
        for action in &parsed.actions {
            // Skip Response actions in first pass - we'll handle them after tools
            if matches!(action, Action::Response(_)) {
                continue;
            }

            let action_key = match action {
                Action::Bash { command, .. } => (&*command).to_string(),
                Action::Tool { name, params } => format!("{}:{}", name, params),
                Action::Response(_) => continue,
            };

            let retry_count = retry_counts.get(&action_key).copied().unwrap_or(0);

            // Send action message only for bash (for approval), not for tools (cleaner UI)
            // Check if tool requires approval based on role
            let needs_approval = match &action {
                Action::Bash { .. } => role.requires_approval("bash"),
                Action::Tool { name, .. } => role.requires_approval(name.as_ref()),
                Action::Response(_) => false,
            };

            if let Action::Bash { command, .. } = &action {
                let _ = tx.send(AgentUpdate::ActionMessage(format!("bash: {}", command).into()));
            } else if let Action::Tool { name, params } = &action {
                let params_str = params.to_string();
                let _ = tx.send(AgentUpdate::ActionMessage(format!("{}: {}", name, params_str).into()));
            }

            // Execute the action
            let result: Result<(Arc<String>, Option<Arc<String>>), String> = match action {
                Action::Bash { command, .. } => {
                    if !needs_approval {
                        execute_bash_action(&*command)
                            .map(|o| (Arc::new(o), None))
                            .map_err(|e| e.to_string())
                    } else {
                        let _ = tx.send(AgentUpdate::Done);
                        return;
                    }
                }
                Action::Tool { name, params } => {
                    // Check if tool is available in current role
                    if !role.tool_available(name.as_ref()) {
                        let error_msg = format!("Tool '{}' is not available in {} mode. Use Shift+Tab to change roles.", name, role.name());
                        let _ = tx.send(AgentUpdate::ErrorMessage(error_msg.clone().into()));
                        conversation.push(Message {
                            role: "assistant".into(),
                            content: Arc::new(crate::tui_components::clean_for_conversation(&lm_output)),
                            images: Vec::new(),
                        });
                        conversation.push(Message {
                            role: "user".into(),
                            content: Arc::new(error_msg),
                            images: Vec::new(),
                        });
                        continue;
                    }

                    // Execute tool and get both full content and display summary
                    tool_registry
                        .execute(name.as_ref(), params.clone())
                        .map(|o| {
                            // Full content for LLM, display summary for UI
                            let llm_content: Arc<String> = o.content;
                            let ui_summary: Option<Arc<String>> = o.content_for_display;
                            (llm_content, ui_summary)
                        })
                        .map_err(|e| e.to_string())
                }
                Action::Response(_) => continue,
            };

            match result {
                Ok((llm_output_content, _ui_display)) => {
                    // Only ActionMessage is shown for tools (no summary output)
                    // Bash commands still show output
                    if let Action::Bash { .. } = &action {
                        let _ = tx.send(AgentUpdate::OutputMessage(llm_output_content.clone()));
                    }

                    // Clean the lm_output before adding to conversation (remove tool/thinking tags)
                    let cleaned_output = crate::tui_components::clean_for_conversation(&lm_output);

                    conversation.push(Message {
                        role: "assistant".into(),
                        content: Arc::new(cleaned_output),
                        images: Vec::new(),
                    });
                    conversation.push(Message {
                        role: "user".into(),
                        content: Arc::new(format!("Tool result:\n{}", &*llm_output_content)),
                        images: Vec::new(),
                    });

                    // Reset retry count on success
                    retry_counts.insert(action_key, 0);
                }
                Err(error_msg) => {
                    if retry_count < max_retries {
                        // Retry with error feedback
                        retry_counts.insert(action_key, retry_count + 1);

                        let error_json = serde_json::json!({
                            "error": "TOOL_ERROR",
                            "message": error_msg,
                            "retry_count": retry_count + 1,
                            "max_retries": max_retries
                        });

                        let _ = tx.send(AgentUpdate::OutputMessage(format!(
                            "Error: {} (retry {}/{})",
                            error_msg,
                            retry_count + 1,
                            max_retries
                        ).into()));
                        conversation.push(Message {
                            role: "assistant".into(),
                            content: Arc::new(crate::tui_components::clean_for_conversation(&lm_output)),
                            images: Vec::new(),
                        });
                        conversation.push(Message {
                            role: "user".into(),
                            content: Arc::new(format!(
                                "Error: {}\n\n{}",
                                error_json, "Please fix the parameters and try again."
                            )),
                            images: Vec::new(),
                        });
                    } else {
                        // Max retries exceeded
                        let _ = tx.send(AgentUpdate::OutputMessage(format!(
                            "Error after {} retries: {}",
                            max_retries, error_msg
                        ).into()));
                        conversation.push(Message {
                            role: "assistant".into(),
                            content: Arc::new(crate::tui_components::clean_for_conversation(&lm_output)),
                            images: Vec::new(),
                        });
                        conversation.push(Message {
                            role: "user".into(),
                            content: Arc::new(format!(
                                "Tool execution failed after {} retries: {}",
                                max_retries, error_msg
                            )),
                            images: Vec::new(),
                        });
                    }
                }
            }
        }

        // After all tools/bash actions complete, check if there was a Response action
        // This handles the case where LLM outputs both tool calls and explanatory text
        if let Some(Action::Response(response)) = parsed.actions.iter().find(|a| matches!(a, Action::Response(_))) {
            let _ = tx.send(AgentUpdate::Done);
            conversation.push(Message {
                role: "assistant".into(),
                content: Arc::new(crate::tui_components::clean_for_conversation(&lm_output)),
                images: Vec::new(),
            });
            conversation.push(Message {
                role: "user".into(),
                content: response.clone(),
                images: Vec::new(),
            });
            return;
        }

        // If we processed tool actions but there's no Response action, continue the loop
        // to get the next LLM response
    }
}

pub struct App {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub cursor_position: usize,
    pub should_quit: bool,
    pub is_processing: bool,
    pub model: Model,
    pub tool_registry: Arc<ToolRegistry>,
    pub tools_description: String,
    pub system_messages: Vec<Message>,
    pub pending_action: Option<PendingAction>,
    pub approval_state: ApprovalState,
    // Simple accumulator for agent response (displayed when done)
    response_accumulator: String,
    // Accumulator for thinking content
    thinking_accumulator: String,
    pub session_input_tokens: u32,
    pub session_output_tokens: u32,
    pub session_cost: f64,
    pub model_name: String,
    // Role system (Yolo/Safe/Planning)
    pub role: Role,
    // New scrolling state
    scroll_state: ScrollState,
    // Selection state
    pub selection: SelectionState,
    // Toast notifications
    toasts: Vec<Toast>,
    pub last_ctrl_c: Option<Instant>,
    pub yolo_mode: bool,
    // Channel to receive updates from background agent task
    update_rx: Option<mpsc::UnboundedReceiver<AgentUpdate>>,
    // Cancellation token to abort the background agent task
    cancel_token: CancellationToken,
    // Store rendered lines for click-to-copy (last rendered state)
    rendered_lines: Vec<String>,
    // Store conversation area rect for click handling
    conversation_area: Option<Rect>,
    // Track last mouse position for drag selection
    last_mouse_pos: Option<(u16, u16)>,
    // Track if mouse is being dragged
    is_mouse_dragging: bool,
    // Prompt history
    history: Vec<String>,
    history_index: Option<usize>,
    // Animation frame counter for UI effects
    animation_frame: usize,
    // Track parallel tool execution
    parallel_tool_count: usize,
    // File picker state for @ symbol file injection
    file_picker_active: bool,
    file_picker_query: String,
    file_picker_results: Vec<String>,
    file_picker_selected: usize,
}

impl App {
    pub fn new(
        model: Model,
        tool_registry: ToolRegistry,
        model_name: String,
        yolo_mode: bool,
    ) -> Self {
        let role = if yolo_mode { Role::Yolo } else { Role::Safe };
        let tools_description = tool_registry.descriptions_for_role(role);
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor_position: 0,
            should_quit: false,
            is_processing: false,
            model,
            tool_registry: Arc::new(tool_registry),
            tools_description,
            system_messages: Vec::new(),
            pending_action: None,
            approval_state: ApprovalState::None,
            response_accumulator: String::new(),
            thinking_accumulator: String::new(),
            session_input_tokens: 0,
            session_output_tokens: 0,
            session_cost: 0.0,
            model_name,
            role,
            scroll_state: ScrollState::new(),
            selection: SelectionState::new(),
            toasts: Vec::new(),
            last_ctrl_c: None,
            yolo_mode,
            update_rx: None,
            cancel_token: CancellationToken::new(),
            rendered_lines: Vec::new(),
            conversation_area: None,
            last_mouse_pos: None,
            is_mouse_dragging: false,
            history: Vec::new(),
            history_index: None,
            animation_frame: 0,
            parallel_tool_count: 0,
            file_picker_active: false,
            file_picker_query: String::new(),
            file_picker_results: Vec::new(),
            file_picker_selected: 0,
        }
    }

    pub fn set_system_prompt(&mut self, system_prompt: &str) {
        self.system_messages = vec![Message {
            role: "system".into(),
            content: Arc::new(system_prompt.to_string()),
            images: Vec::new(),
        }];
    }

    /// Update the role and regenerate tools description
    pub fn set_role(&mut self, role: Role) {
        self.role = role;
        self.tools_description = self.tool_registry.descriptions_for_role(role);
        // Update yolo_mode based on role for compatibility
        self.yolo_mode = matches!(role, Role::Yolo);
    }

    /// Cycle to the next role (Planning -> Safe -> Yolo -> Planning)
    pub fn cycle_role(&mut self) {
        let new_role = match self.role {
            Role::Planning => Role::Safe,
            Role::Safe => Role::Yolo,
            Role::Yolo => Role::Planning,
        };
        self.set_role(new_role);
    }

    fn get_conversation_history(&self) -> Vec<Message> {
        let mut messages = self.system_messages.clone();
        for chat_msg in &self.messages {
            // Include user, assistant, and output messages for the LLM
            // Output messages contain tool results which the LLM needs to see
            match &*chat_msg.role {
                "user" | "assistant" | "output" => {
                    messages.push(Message {
                        role: if &*chat_msg.role == "output" { "user".into() } else { chat_msg.role.clone() },
                        content: chat_msg.content.clone(),
                        images: Vec::new(),
                    });
                }
                _ => {}
            }
        }
        messages
    }
}

impl App {
    /// Start processing a message in the background (non-blocking).
    pub fn start_processing(&mut self) {
        if self.input.trim().is_empty() {
            return;
        }

        let user_input = self.input.clone();

        // Add to history (avoid duplicates)
        if self.history.last() != Some(&user_input) {
            self.history.push(user_input.clone());
        }

        // Reset history index when submitting new input
        self.history_index = None;

        self.messages.push(ChatMessage::user(&user_input));
        self.input.clear();
        self.cursor_position = 0;
        self.is_processing = true;
        self.response_accumulator.clear();
        self.thinking_accumulator.clear();
        self.scroll_state.scroll_to_bottom();
        self.clear_selection();

        let conversation = self.get_conversation_history();
        let model = self.model.clone();
        let tool_registry = Arc::clone(&self.tool_registry);
        let tools_description = self.tools_description.clone();
        let role = self.role;

        let (tx, rx) = mpsc::unbounded_channel();
        self.update_rx = Some(rx);

        let cancel_token = CancellationToken::new();
        self.cancel_token = cancel_token.clone();

        tokio::spawn(async move {
            agent_loop(
                model,
                tool_registry,
                tools_description,
                conversation,
                tx,
                cancel_token,
                role,
            )
            .await;
        });
    }

    /// Drain pending updates from the background task channel.
    fn poll_updates(&mut self) {
        let mut should_clear_selection = false;
        let mut should_clear_rx = false;
        let mut should_return = false;
        let mut pending_action: Option<(PendingAction, ApprovalState)> = None;

        {
            let rx = match &mut self.update_rx {
                Some(rx) => rx,
                None => return,
            };

            loop {
                match rx.try_recv() {
                    Ok(update) => match update {
                        AgentUpdate::StreamingChunk(chunk) => {
                            // Simply accumulate content, display when done
                            self.response_accumulator.push_str(&chunk);
                        }
                        AgentUpdate::StreamingThinkingChunk(thinking) => {
                            // Accumulate thinking content for display
                            self.thinking_accumulator.push_str(&thinking);
                        }
                        AgentUpdate::ActionMessage(content) => {
                            // Flush any accumulated content (with thinking) before showing the action
                            let thinking = if !self.thinking_accumulator.is_empty() {
                                Some(self.thinking_accumulator.clone())
                            } else {
                                None
                            };
                            if !self.response_accumulator.is_empty() {
                                self.messages.push(
                                    ChatMessage::assistant_with_explicit_thinking(&self.response_accumulator, thinking),
                                );
                                self.response_accumulator.clear();
                                self.thinking_accumulator.clear();
                            }
                            self.messages.push(ChatMessage::action(&content));
                            self.scroll_state.scroll_to_bottom();
                            should_clear_selection = true;
                            // Check if this is a bash action that needs approval (based on role)
                            if self.role.requires_approval("bash") && content.starts_with("bash:") {
                                // Extract command from the action display
                                let command = content.strip_prefix("bash: ").unwrap_or(&content);
                                let action = PendingAction {
                                    action_type: "bash".into(),
                                    content: Arc::new(command.to_string()),
                                };
                                pending_action = Some((action, ApprovalState::Pending));
                                self.is_processing = false;
                                // Cancel the background task
                                self.cancel_token.cancel();
                                should_clear_rx = true;
                                should_return = true;
                            }
                        }
                        AgentUpdate::OutputMessage(content) => {
                            // Flush any accumulated content (with thinking) before showing output
                            let thinking = if !self.thinking_accumulator.is_empty() {
                                Some(self.thinking_accumulator.clone())
                            } else {
                                None
                            };
                            if !self.response_accumulator.is_empty() {
                                self.messages.push(
                                    ChatMessage::assistant_with_explicit_thinking(&self.response_accumulator, thinking),
                                );
                                self.response_accumulator.clear();
                                self.thinking_accumulator.clear();
                            }
                            self.messages.push(ChatMessage::output(&*content));
                            self.scroll_state.scroll_to_bottom();
                            should_clear_selection = true;
                        }
                        AgentUpdate::ErrorMessage(content) => {
                            // Flush any accumulated content (with thinking) before showing error
                            let thinking = if !self.thinking_accumulator.is_empty() {
                                Some(self.thinking_accumulator.clone())
                            } else {
                                None
                            };
                            if !self.response_accumulator.is_empty() {
                                self.messages.push(
                                    ChatMessage::assistant_with_explicit_thinking(&self.response_accumulator, thinking),
                                );
                                self.response_accumulator.clear();
                                self.thinking_accumulator.clear();
                            }
                            self.messages.push(ChatMessage::error(&content));
                            self.scroll_state.scroll_to_bottom();
                            should_clear_selection = true;
                        }
                        AgentUpdate::SystemMessage(content) => {
                            self.messages.push(ChatMessage::system(&content));
                            self.scroll_state.scroll_to_bottom();
                            should_clear_selection = true;
                        }
                        AgentUpdate::UsageUpdate { input_tokens, output_tokens, cost } => {
                            self.session_input_tokens += input_tokens;
                            self.session_output_tokens += output_tokens;
                            self.session_cost += cost;
                        }
                        AgentUpdate::ParallelToolCount(count) => {
                            self.parallel_tool_count = count;
                        }
                        AgentUpdate::Done => {
                            // Flush any remaining accumulated content
                            if !self.response_accumulator.is_empty() {
                                self.messages.push(
                                    ChatMessage::assistant(&self.response_accumulator),
                                );
                                self.response_accumulator.clear();
                            }
                            self.is_processing = false;
                            self.parallel_tool_count = 0;
                            should_clear_rx = true;
                            should_return = true;
                        }
                    },
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        // Flush any remaining accumulated content with thinking
                        let thinking = if !self.thinking_accumulator.is_empty() {
                            Some(self.thinking_accumulator.clone())
                        } else {
                            None
                        };
                        if !self.response_accumulator.is_empty() {
                            self.messages
                                .push(ChatMessage::assistant_with_explicit_thinking(&self.response_accumulator, thinking));
                            self.response_accumulator.clear();
                            self.thinking_accumulator.clear();
                        }
                        self.is_processing = false;
                        self.parallel_tool_count = 0;
                        should_clear_rx = true;
                        should_return = true;
                    }
                }
                if should_return {
                    break;
                }
            }
        }

        // Execute deferred actions outside the borrow scope
        if should_clear_selection {
            self.clear_selection();
        }
        if should_clear_rx {
            self.update_rx = None;
        }
        if let Some((action, approval)) = pending_action {
            self.pending_action = Some(action);
            self.approval_state = approval;
        }
    }

    /// Cancel the current processing.
    pub fn cancel_processing(&mut self) {
        // Try to interrupt server-side generation (for llama.cpp)
        let model = self.model.clone();
        tokio::spawn(async move {
            let _ = model.interrupt_generation().await;
        });

        self.cancel_token.cancel();
        self.is_processing = false;
        // Add a visible cancellation message
        if !self.response_accumulator.is_empty() {
            self.messages.push(ChatMessage::system("Request cancelled"));
        }
        // Clear any pending response
        self.response_accumulator.clear();
        self.thinking_accumulator.clear();
        // Drop the update_rx to stop processing further updates
        self.update_rx = None;
        // Show a toast notification
        self.show_toast("Cancelled".to_string());
    }

    // ── History Navigation Methods ───────────────────────────────────

    /// Navigate to the previous entry in history (Up arrow)
    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }

        // If not currently browsing history, save current input
        if self.history_index.is_none() {
            self.history_index = Some(self.history.len());
        }

        if let Some(idx) = self.history_index
            && idx > 0 {
                self.history_index = Some(idx - 1);
                if let Some(entry) = self.history.get(idx - 1) {
                    self.input = entry.clone();
                    self.cursor_position = self.input.len();
                }
            }
    }

    /// Navigate to the next entry in history (Down arrow)
    pub fn history_next(&mut self) {
        if self.history.is_empty() {
            return;
        }

        if let Some(idx) = self.history_index
            && idx < self.history.len() {
                self.history_index = Some(idx + 1);
                if idx + 1 < self.history.len() {
                    if let Some(entry) = self.history.get(idx + 1) {
                        self.input = entry.clone();
                        self.cursor_position = self.input.len();
                    }
                } else {
                    // At the end of history, clear input (return to editing mode)
                    self.input.clear();
                    self.cursor_position = 0;
                    self.history_index = None;
                }
            }
    }

    /// Exit history browsing mode (called when user edits input manually)
    pub fn exit_history_mode(&mut self) {
        self.history_index = None;
    }

    // ── File Picker Methods ───────────────────────────────────────────

    /// Activate file picker and populate with files from current directory
    pub fn activate_file_picker(&mut self, query: String) {
        self.file_picker_active = true;
        self.file_picker_query = query.clone();
        self.file_picker_selected = 0;

        // Get files from current directory
        let files = self.get_files_in_current_dir();

        // Filter by query if provided
        if query.is_empty() {
            self.file_picker_results = files;
        } else {
            self.file_picker_results = crate::fuzzy::fuzzy_filter(&files, &query)
                .into_iter()
                .map(|(idx, _)| files[idx].clone())
                .collect();
        }
    }

    /// Get list of files in current directory (recursive)
    fn get_files_in_current_dir(&self) -> Vec<String> {
        use std::fs;

        let mut files = Vec::new();

        // Recursive directory walk
        if let Ok(_entries) = fs::read_dir(".") {
            fn visit_dir(path: std::path::PathBuf, files: &mut Vec<String>) {
                if let Ok(entries) = fs::read_dir(&path) {
                    for entry in entries.filter_map(|e| e.ok()) {
                        let entry_path = entry.path();
                        let name = entry.file_name().to_string_lossy().to_string();

                        // Skip hidden files and common ignore patterns
                        if name.starts_with('.') {
                            continue;
                        }

                        if entry_path.is_dir() {
                            files.push(format!("{}/", name));
                            visit_dir(entry_path, files);
                        } else {
                            files.push(name);
                        }
                    }
                }
            }

            visit_dir(std::path::PathBuf::from("."), &mut files);
        }

        files
    }

    /// Deactivate file picker
    pub fn deactivate_file_picker(&mut self) {
        self.file_picker_active = false;
        self.file_picker_query.clear();
        self.file_picker_results.clear();
        self.file_picker_selected = 0;
    }

    /// Select next file in picker (with wrapping)
    pub fn file_picker_next(&mut self) {
        if !self.file_picker_results.is_empty() {
            self.file_picker_selected = (self.file_picker_selected + 1) % self.file_picker_results.len();
        }
    }

    /// Select previous file in picker (with wrapping)
    pub fn file_picker_prev(&mut self) {
        if !self.file_picker_results.is_empty() {
            self.file_picker_selected = if self.file_picker_selected == 0 {
                self.file_picker_results.len() - 1
            } else {
                self.file_picker_selected - 1
            };
        }
    }

    /// Get the currently selected file from picker
    pub fn get_selected_file(&self) -> Option<String> {
        self.file_picker_results.get(self.file_picker_selected).cloned()
    }

    // ── Selection & Copy Methods ─────────────────────────────────────

    /// Clear the current selection
    pub fn clear_selection(&mut self) {
        self.selection = SelectionState::new();
    }

    /// Start a selection at the given line index
    pub fn start_selection(&mut self, line_index: usize) {
        self.selection.mode = SelectionMode::Selecting;
        self.selection.start_line = line_index;
        self.selection.end_line = line_index;
        self.selection.anchor_line = line_index;
    }

    /// Update the selection end to the given line index
    pub fn update_selection(&mut self, line_index: usize) {
        self.selection.end_line = line_index;
        self.selection.mode = SelectionMode::Selected;
    }

    /// Copy the current selection to clipboard
    pub fn copy_selection(&mut self) {
        if !self.selection.is_active() {
            return;
        }

        let (start, end) = self.selection.get_range();
        if start >= self.rendered_lines.len() || end >= self.rendered_lines.len() {
            return;
        }

        let lines_to_copy: Vec<&str> = self.rendered_lines[start..=end]
            .iter()
            .map(|s| s.as_str())
            .collect();

        let text_to_copy = lines_to_copy.join("\n");
        if !text_to_copy.is_empty() {
            let success = copy_to_clipboard(&text_to_copy);
            let line_count = end - start + 1;
            if success {
                self.show_toast(format!(
                    "Copied {} line{}",
                    line_count,
                    if line_count == 1 { "" } else { "s" }
                ));
            } else {
                self.show_toast(
                    "Copy failed - install xclip (Linux) or use mouse selection".to_string(),
                );
            }
        }

        self.clear_selection();
    }

    /// Copy a single line by index
    pub fn copy_line(&mut self, line_index: usize) {
        if line_index >= self.rendered_lines.len() {
            return;
        }

        let text = &self.rendered_lines[line_index];
        if !text.trim().is_empty() {
            let success = copy_to_clipboard(text.trim());
            if success {
                self.show_toast("Copied 1 line".to_string());
            } else {
                self.show_toast(
                    "Copy failed - install xclip (Linux) or use mouse selection".to_string(),
                );
            }
        }
    }

    /// Show a toast notification
    pub fn show_toast(&mut self, message: String) {
        self.toasts.push(Toast::new(message));
    }

    /// Clean up expired toasts
    pub fn cleanup_toasts(&mut self) {
        self.toasts.retain(|t| !t.is_expired());
    }

    /// Get the line index at the given screen coordinates
    pub fn line_index_at_coords(&self, column: u16, row: u16) -> Option<usize> {
        let area = self.conversation_area?;

        // Check if click is within conversation area bounds
        if column < area.left()
            || column >= area.right()
            || row < area.top()
            || row >= area.bottom()
        {
            return None;
        }

        // Calculate which row within the area (0 = first row of content)
        let row_in_area = (row - area.top()) as usize;

        // Calculate line index: offset + row_in_area
        // (No border adjustment since we don't render borders on the Paragraph)
        let line_index = self.scroll_state.offset + row_in_area;

        if line_index < self.rendered_lines.len() {
            Some(line_index)
        } else {
            None
        }
    }

    /// Handle a mouse click in the conversation area
    pub fn handle_click(&mut self, column: u16, row: u16) {
        if let Some(line_index) = self.line_index_at_coords(column, row) {
            self.start_selection(line_index);
            self.last_mouse_pos = Some((column, row));
            self.is_mouse_dragging = false;
        } else {
            self.clear_selection();
        }
    }

    /// Handle mouse drag for selection
    pub fn handle_drag(&mut self, column: u16, row: u16) {
        self.is_mouse_dragging = true;
        if let Some(line_index) = self.line_index_at_coords(column, row) {
            self.update_selection(line_index);
        }
        self.last_mouse_pos = Some((column, row));
    }

    /// Handle mouse release - finish selection and copy
    pub fn handle_release(&mut self) {
        if self.is_mouse_dragging && self.selection.is_active() {
            // Always copy immediately on mouse release
            self.copy_selection();
        } else if self.selection.is_active() {
            // Single click - copy the line immediately
            let line_index = self.selection.start_line;
            self.copy_line(line_index);
            self.clear_selection();
        }
        self.is_mouse_dragging = false;
        self.last_mouse_pos = None;
    }

    pub async fn continue_after_approval(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Take the pending action and current approval state
        let pending = self.pending_action.take();
        let approval = self.approval_state.clone();
        self.approval_state = ApprovalState::None;

        let mut conversation = self.get_conversation_history();

        if let Some(last_msg) = self.messages.last()
            && &*last_msg.role == "action"
        {
            conversation.push(Message {
                role: "assistant".into(),
                content: last_msg.content.clone(),
                images: Vec::new(),
            });
        }

        match approval {
            ApprovalState::Approved => {
                if let Some(PendingAction { content: cmd, .. }) = pending {
                    // Execute bash command asynchronously to avoid blocking the event loop
                    match execute_bash_action_async(&*cmd).await {
                        Ok(output) => {
                            self.messages.push(ChatMessage::output(&output));
                            conversation.push(Message {
                                role: "user".into(),
                                content: Arc::new(output),
                                images: Vec::new(),
                            });
                        }
                        Err(e) => {
                            let error_msg = format!("Error: {}", e);
                            self.messages.push(ChatMessage::error(&error_msg));
                            conversation.push(Message {
                                role: "user".into(),
                                content: Arc::new(error_msg),
                                images: Vec::new(),
                            });
                        }
                    }
                }
            }
            ApprovalState::Rejected => {
                self.messages
                    .push(ChatMessage::output("Action rejected by user"));
                conversation.push(Message {
                    role: "user".into(),
                    content: Arc::new("Action rejected".to_string()),
                    images: Vec::new(),
                });
            }
            _ => {}
        }

        // Restart the agent loop in background
        self.is_processing = true;
        let model = self.model.clone();
        let tool_registry = Arc::clone(&self.tool_registry);
        let tools_description = self.tools_description.clone();
        let role = self.role;

        let (tx, rx) = mpsc::unbounded_channel();
        self.update_rx = Some(rx);

        let cancel_token = CancellationToken::new();
        self.cancel_token = cancel_token.clone();

        tokio::spawn(async move {
            agent_loop(
                model,
                tool_registry,
                tools_description,
                conversation,
                tx,
                cancel_token,
                role,
            )
            .await;
        });

        Ok(())
    }
}

async fn execute_bash_action_async(command: &str) -> Result<String, AgentError> {
    if command.trim() == "exit" {
        return Err(AgentError::Terminating(
            "Agent requested to exit".to_string(),
        ));
    }

    let output = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(command)
        .env("PAGER", "cat")
        .env("MANPAGER", "cat")
        .env("LESS", "-R")
        .env("PIP_PROGRESS_BAR", "off")
        .env("TQDM_DISABLE", "1")
        .output()
        .await;

    match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout).to_string();
            let stderr = String::from_utf8_lossy(&result.stderr).to_string();
            Ok(format!("{}\n{}", stdout, stderr).trim().to_string())
        }
        Err(e) => Err(AgentError::Timeout(format!(
            "Command execution failed: {}",
            e
        ))),
    }
}

fn execute_bash_action(command: &str) -> Result<String, AgentError> {
    use std::process::Command;

    if command.trim() == "exit" {
        return Err(AgentError::Terminating(
            "Agent requested to exit".to_string(),
        ));
    }

    let output = Command::new("bash")
        .arg("-c")
        .arg(command)
        .env("PAGER", "cat")
        .env("MANPAGER", "cat")
        .env("LESS", "-R")
        .env("PIP_PROGRESS_BAR", "off")
        .env("TQDM_DISABLE", "1")
        .output();

    match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout).to_string();
            let stderr = String::from_utf8_lossy(&result.stderr).to_string();
            Ok(format!("{}\n{}", stdout, stderr).trim().to_string())
        }
        Err(e) => Err(AgentError::Timeout(format!(
            "Command execution failed: {}",
            e
        ))),
    }
}

fn draw_banner(f: &mut Frame, app: &App, area: Rect) {
    if let Some(logo_lines) = logo::get_logo() {
        // Get the actual display width (columns) of the logo, not just character count
        let logo_width = logo_lines.first()
            .map(|l| l.width())
            .unwrap_or(0);
        let logo_height = logo_lines.len();

        // Get directory name in "parent/current" format
        let current_dir = std::env::current_dir()
            .ok()
            .and_then(|p| {
                let parent = p.parent().and_then(|par| par.file_name().and_then(|n| n.to_str()));
                let current = p.file_name().and_then(|n| n.to_str());
                match (parent, current) {
                    (Some(p), Some(c)) => Some(format!("{}/{}", p, c)),
                    (_, Some(c)) => Some(c.to_string()),
                    _ => None,
                }
            })
            .unwrap_or_else(|| "~".to_string());

        // Build all text lines (4 lines total)
        let text_lines = vec![
            Line::from(vec![
                Span::styled("codr", Style::default().fg(Color::Rgb(147, 197, 253)).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("v0.1.0", Style::default().fg(Color::Rgb(100, 100, 110))),
            ]),
            Line::from(vec![
                Span::styled(&app.model_name, Style::default().fg(Color::Rgb(180, 180, 190))),
            ]),
            Line::from(vec![
                Span::styled(&current_dir, Style::default().fg(Color::Rgb(120, 120, 140))),
            ]),
        ];

        // Split: logo (left) | gap (1 col) | text (right)
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(logo_width as u16),  // logo exact width
                Constraint::Length(1),                   // 1 column gap
                Constraint::Min(0),                      // text
            ])
            .split(area);

        // Draw logo (top-aligned, full height)
        let logo_area = ratatui::layout::Rect {
            x: chunks[0].x,
            y: chunks[0].y,
            width: logo_width as u16,
            height: (logo_height as u16).min(chunks[0].height),
        };

        let logo_content: Vec<Line> = logo_lines.iter()
            .take(logo_area.height as usize)
            .map(|line| Line::from(Span::styled(
                line.as_str(),
                Style::default().fg(Color::Rgb(180, 100, 255)),
            )))
            .collect();

        let logo_widget = Paragraph::new(logo_content);
        f.render_widget(logo_widget, logo_area);

        // Draw text (moved down by 1 line)
        let text_area = ratatui::layout::Rect {
            x: chunks[2].x,
            y: chunks[2].y + 1,  // Move down by 1 line
            width: chunks[2].width,
            height: chunks[2].height.saturating_sub(1),
        };

        let text_widget = Paragraph::new(text_lines);
        f.render_widget(text_widget, text_area);
    }
}

// ── UI Drawing ───────────────────────────────────────────────

fn draw_ui(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // Check if logo exists and we should show banner (no messages yet)
    let logo_dims = logo::get_logo_dimensions();
    let show_banner = logo_dims.is_some() && app.messages.is_empty();

    // Calculate banner height: max of logo height or 4 (text lines)
    let banner_height = logo_dims
        .map(|(_, h)| h as u16)
        .unwrap_or(0)
        .max(4);

    // Responsive split: banner (optional) | conversation | input area | subtle footer status line
    let zones = if show_banner {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(banner_height), // banner (logo + text)
                Constraint::Min(1),    // conversation
                Constraint::Length(2), // spacer (2 lines for rainbow + blank line)
                Constraint::Length(2), // input
                Constraint::Length(1), // subtle footer
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // conversation
                Constraint::Length(2), // spacer (2 lines for rainbow + blank line)
                Constraint::Length(2), // input
                Constraint::Length(1), // subtle footer
            ])
            .split(area)
    };

    // ── Banner (shown only on initial screen with logo) ───────
    if show_banner {
        draw_banner(f, app, zones[0]);
        // ── Conversation area ────────────────────────────────────
        draw_conversation(f, app, zones[1]);
        // ── Input area ───────────────────────────────────────────
        draw_input(f, app, zones[3]);
        // ── Footer bar ───────────────────────────────────────────
        draw_footer(f, app, zones[4]);
        // ── Rainbow Working indicator (in spacer above input) ─────────
        if app.is_processing {
            draw_working_indicator(f, app, zones[2]);
        }

        // ── File Picker Popup (if active) ─────────────────────────────
        if app.file_picker_active {
            draw_file_picker(f, app);
        }
    } else {
        // ── Conversation area ────────────────────────────────────
        draw_conversation(f, app, zones[0]);
        // ── Input area ───────────────────────────────────────────
        draw_input(f, app, zones[2]);
        // ── Footer bar ───────────────────────────────────────────
        draw_footer(f, app, zones[3]);
        // ── Rainbow Working indicator (in spacer above input) ─────────
        if app.is_processing {
            draw_working_indicator(f, app, zones[1]);
        }

        // ── File Picker Popup (if active) ─────────────────────────────
        if app.file_picker_active {
            draw_file_picker(f, app);
        }
    }
}


fn draw_footer(f: &mut Frame, app: &mut App, area: Rect) {
    // Clean up expired toasts
    app.cleanup_toasts();

    // Get current memory usage (RSS in MB)
    let memory_mb = get_process_memory_mb();

    // Get role color
    let (role_r, role_g, role_b) = app.role.color();

    // Scroll indicators
    let scroll_pct = app.scroll_state.scroll_percentage();
    let scroll_indicator = if !app.scroll_state.is_at_bottom() && app.scroll_state.total_lines > 0 {
        format!(" {}%", scroll_pct as u32)
    } else {
        String::new()
    };
    let lock_indicator = if !app.scroll_state.auto_scroll {
        " ◍"
    } else {
        ""
    };

    // Build footer spans incrementally to handle width constraints
    let mut spans = Vec::new();

    // Left: codr branding with model name
    spans.push(Span::styled(
        "codr ",
        Style::default()
            .fg(Color::Rgb(147, 197, 253))
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        &app.model_name,
        Style::default()
            .fg(Color::Rgb(147, 197, 253))
            .add_modifier(Modifier::BOLD),
    ));

    // Role badge
    spans.push(Span::styled(" [", Style::default().fg(Color::Rgb(147, 197, 253))));
    spans.push(Span::styled(
        app.role.name(),
        Style::default()
            .fg(Color::Rgb(role_r, role_g, role_b))
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled("]", Style::default().fg(Color::Rgb(147, 197, 253))));

    // Calculate remaining width for right side
    let left_width: usize = spans.iter().map(|s| s.content.width()).sum();
    let available_width = area.width.saturating_sub(left_width as u16 + 2) as usize;

    // Build right side text
    let is_pending = matches!(app.approval_state, ApprovalState::Pending);
    let right_text = if is_pending || app.is_processing {
        format!("{}MB{}{}  ", memory_mb, scroll_indicator, lock_indicator)
    } else {
        format!("in:{} out:{} ${:.4}  {}MB{}{}  ",
            app.session_input_tokens, app.session_output_tokens, app.session_cost, memory_mb, scroll_indicator, lock_indicator)
    };

    // Add padding and right side (with truncation if needed)
    let right_display = if right_text.width() > available_width {
        // Truncate from the left (keep the cost info at the end)
        let truncated: String = right_text.chars().rev()
            .collect::<String>()
            .chars().take(available_width.saturating_sub(3))
            .collect::<String>()
            .chars().rev()
            .collect();
        format!("...{}", truncated)
    } else {
        let padding = " ".repeat(available_width.saturating_sub(right_text.width()));
        format!("{}{}", padding, right_text)
    };

    // Build complete footer with right side
    spans.push(Span::styled(
        right_display,
        Style::default().fg(Color::Rgb(163, 165, 170)),
    ));

    let footer = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::Rgb(28, 28, 30)));

    f.render_widget(footer, area);

    // Draw toast notifications if any
    if let Some(toast) = app.toasts.first() {
        let toast_y = area
            .bottom()
            .min(f.area().bottom())
            .saturating_sub(4);
        let toast_area = Rect::new(
            area.right().saturating_sub(toast.message.len() as u16 + 4),
            toast_y,
            toast.message.len() as u16 + 4,
            1,
        );
        let toast_paragraph = Paragraph::new(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                &toast.message,
                Style::default()
                    .fg(Color::Rgb(134, 239, 172)) // Bright green for success
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .style(Style::default().bg(Color::Rgb(46, 46, 48))); // Toast background
        f.render_widget(toast_paragraph, toast_area);
    }
}

/// Get current process memory usage in MB (RSS)
fn get_process_memory_mb() -> u64 {
    // Use the libc-based approach for cross-platform memory reading
    #[cfg(unix)]
    {
        use std::fs;

        if let Ok(status) = fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    // VmRSS:     12345 kB
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            return kb / 1024; // Convert to MB
                        }
                    }
                }
            }
        }
        // Fallback for systems without /proc (like macOS)
        0
    }

    #[cfg(not(unix))]
    {
        0 // Not implemented for non-Unix systems
    }
}

/// Draw rainbow "Working..." indicator in the spacer above input
fn draw_working_indicator(f: &mut Frame, app: &App, spacer_area: Rect) {
    // Draw at the top of the spacer, leaving a blank line below
    let indicator_area = Rect {
        x: spacer_area.x,
        y: spacer_area.y,
        width: spacer_area.width,
        height: 1,
    };

    // Animated rainbow colors (left-to-right gradient sweep)
    let frame = app.animation_frame;
    let text = if app.parallel_tool_count > 1 {
        format!("Running {} tools in parallel{}", app.parallel_tool_count, ".".repeat((frame / 8) % 4))
    } else {
        format!("Working{}", ".".repeat((frame / 8) % 4))
    };

    // Create rainbow gradient spans (left-aligned over prompt)
    let mut spans = Vec::new();

    // Text with gradient effect (left-aligned)
    for (i, c) in text.chars().enumerate() {
        let progress = ((frame as i32 + i as i32 * 8).rem_euclid(256)) as u8;
        let (r, g, b) = rainbow_color(progress);
        spans.push(Span::styled(
            c.to_string(),
            Style::default()
                .fg(Color::Rgb(r, g, b))
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Right padding (no left padding to keep it aligned with prompt)
    let width = indicator_area.width as usize;
    let text_width = text.width();
    for i in 0..width.saturating_sub(text_width) {
        let progress = ((frame + (text_width + i) * 2) % 256) as u8;
        let (r, g, b) = rainbow_color(progress);
        spans.push(Span::styled(" ", Style::default().fg(Color::Rgb(r, g, b))));
    }

    let paragraph = Paragraph::new(Line::from(spans));
    f.render_widget(paragraph, indicator_area);
}

/// Generate rainbow RGB color from 0-255 value (hue-based)
fn rainbow_color(value: u8) -> (u8, u8, u8) {
    let v = value as u32;
    let phase = v * 6;

    let (r, g, b) = match phase {
        0..=255 => {
            // Red to Yellow
            (255, phase as u8, 0)
        }
        256..=511 => {
            // Yellow to Green
            ((511 - phase) as u8, 255, 0)
        }
        512..=767 => {
            // Green to Cyan
            (0, 255, (phase - 512) as u8)
        }
        768..=1023 => {
            // Cyan to Blue
            (0, (1023 - phase) as u8, 255)
        }
        1024..=1279 => {
            // Blue to Magenta
            ((phase - 1024) as u8, 0, 255)
        }
        _ => {
            // Magenta to Red
            (255, 0, (1535 - phase) as u8)
        }
    };
    (r, g, b)
}

fn draw_conversation(f: &mut Frame, app: &mut App, area: Rect) {
    let _t = &*THEME;
    // minimal padding on sides instead of borders
    let width = area.width.saturating_sub(2) as usize;

    // Store conversation area for click handling
    app.conversation_area = Some(area);

    // Update scroll state viewport height
    app.scroll_state.viewport_height = area.height as usize;

    // Render all messages into lines
    let mut all_lines: Vec<Line<'_>> = Vec::new();
    let mut rendered_text: Vec<String> = Vec::new(); // Store raw text for copying

    for msg in &app.messages {
        let rendered = render_message(msg, width);
        for line in &rendered {
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            rendered_text.push(text);
        }
        all_lines.extend(rendered);
    }

    // Store rendered text for click handling
    app.rendered_lines = rendered_text;

    // Update scroll state with total lines
    app.scroll_state
        .update_total_lines(app.rendered_lines.len());

    // Calculate visible lines based on scroll offset
    let visible_height = area.height as usize;

    // Apply selection highlight to visible lines
    let display_lines: Vec<Line<'static>> = all_lines
        .iter()
        .enumerate()
        .skip(app.scroll_state.offset)
        .take(visible_height)
        .map(|(idx, line)| {
            if app.selection.contains_line(idx) {
                // Apply selection highlight
                let selection_style = Style::default()
                    .fg(Color::Rgb(255, 255, 255))
                    .bg(Color::Rgb(80, 120, 180));
                Line::from(
                    line.spans
                        .iter()
                        .map(|span| Span::styled(span.content.to_string(), selection_style))
                        .collect::<Vec<_>>(),
                )
            } else {
                // Convert to owned static lines, preserving style
                Line::from(
                    line.spans
                        .iter()
                        .map(|span| Span::styled(span.content.to_string(), span.style))
                        .collect::<Vec<_>>(),
                )
            }
        })
        .collect();

    // Clear background since ratatui doesn't render empty spans automatically
    let layout_clear = Paragraph::new(display_lines).style(Style::default().bg(ratatui::style::Color::Reset));
    f.render_widget(layout_clear, area);
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let t = &*THEME;
    let is_pending = matches!(app.approval_state, ApprovalState::Pending);

    // Input area: input (2)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(2), // input area (2-line padded buffer)
        ])
        .split(area);

    // Input style
    let input_style = if app.is_processing {
        Style::default().fg(Color::Rgb(115, 115, 120)) // Dim gray when processing
    } else if is_pending {
        Style::default().fg(Color::Rgb(255, 215, 120)) // Warm yellow for approval
    } else {
        Style::default().fg(Color::Rgb(226, 232, 240)) // Soft white for normal input
    };

    // Prompt prefix - model name + indicator
    // Add history indicator if browsing history
    let history_suffix = if let Some(idx) = app.history_index {
        format!(" [{}]", idx + 1)
    } else {
        String::new()
    };

    let (prompt_label, prompt_indicator) = if is_pending {
        ("approve / reject".to_string(), "")
    } else if app.is_processing {
        (app.model_name.clone(), " ◉")
    } else {
        (format!("{}{}", app.model_name, history_suffix), " >")
    };

    // Build input text with prompt
    let prompt_style = if is_pending {
        Style::default()
            .fg(Color::Rgb(255, 215, 120))
            .add_modifier(Modifier::BOLD)
    } else if app.is_processing {
        Style::default().fg(Color::Rgb(100, 100, 110))
    } else {
        t.prompt
    };

    // Prompt span for reuse
    let prompt_span = Span::styled(
        format!("{}{} ", prompt_label, prompt_indicator),
        prompt_style,
    );

    // Wrap input to fit width (accounting for prompt prefix)
    let available_width = chunks[0].width as usize;
    let prefix_width = prompt_label.width() + prompt_indicator.width() + 1;

    // Simple wrapping for display
    let input_text = app.input.as_str();
    let mut display_lines = Vec::new();

    // Determine placeholder text
    let placeholder = if is_pending {
        "[a]pprove or [r]eject"
    } else if input_text.is_empty() {
        "Type your message..."
    } else {
        ""
    };

    let first_line_capacity = available_width.saturating_sub(prefix_width);

    // First line
    if input_text.is_empty() && !placeholder.is_empty() {
        // Render placeholder string
        display_lines.push(Line::from(vec![
            prompt_span.clone(),
            Span::styled(
                placeholder,
                Style::default().fg(Color::Rgb(92, 92, 97)), // Dim placeholder
            ),
        ]));

        // Blank second line
        display_lines.push(Line::from(""));

        let input = Paragraph::new(display_lines);
        f.render_widget(input, chunks[0]);
        // Cursor on placeholder start
        if !app.is_processing && !is_pending {
            f.set_cursor_position((
                chunks[0].x + prefix_width as u16,
                chunks[0].y,
            ));
        }
        return;
    }

    let (first_line, remaining) = if input_text.width() > first_line_capacity {
        // Truncate for first line
        let truncated = truncate_text(input_text, first_line_capacity);
        (truncated, Some(&input_text[truncated.len()..]))
    } else {
        (input_text, None)
    };

    // Calculate padding
    let text_width = first_line.width();
    let padding_width = first_line_capacity.saturating_sub(text_width);
    let padding = " ".repeat(padding_width);

    display_lines.push(Line::from(vec![
        prompt_span.clone(),
        Span::styled(first_line, input_style),
        Span::styled(&padding, Style::default()),
    ]));

    // Second line
    if let Some(remaining_text) = remaining {
        let truncated = truncate_text(remaining_text, available_width);
        display_lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", " ".repeat(prefix_width)),
                Style::default(),
            ),
            Span::styled(truncated, input_style),
        ]));
    } else {
        // Empty second line to maintain 2-line height
        display_lines.push(Line::from(""));
    }

    let input = Paragraph::new(display_lines);
    f.render_widget(input, chunks[0]);

    // Cursor positioning
    if !app.is_processing && !is_pending {
        // Calculate the display width of text up to cursor position
        let text_before_cursor = &app.input[..app.cursor_position.min(app.input.len())];
        let cursor_width = text_before_cursor.width();
        let cursor_offset = cursor_width.min(first_line_capacity);
        if cursor_offset < first_line_capacity {
            // Cursor on first line
            f.set_cursor_position((
                chunks[0].x + prefix_width as u16 + cursor_offset as u16,
                chunks[0].y,
            ));
        } else {
            // Cursor on second line
            let remaining_offset = cursor_width.saturating_sub(first_line_capacity);
            let second_line_capacity = available_width;
            if remaining_offset < second_line_capacity {
                f.set_cursor_position((
                    chunks[0].x + prefix_width as u16 + remaining_offset as u16,
                    chunks[0].y + 1,
                ));
            } else {
                // End of second line
                f.set_cursor_position((
                    chunks[0].x + available_width as u16,
                    chunks[0].y + 1,
                ));
            }
        }
    }
}

// Helper function to truncate text to fit width
fn truncate_text(text: &str, max_width: usize) -> &str {
    let mut current_width = 0;
    for (i, c) in text.char_indices() {
        let char_width = c.width().unwrap_or(0);
        if current_width + char_width > max_width {
            return &text[..i];
        }
        current_width += char_width;
    }
    text
}

// ── Main TUI loop ────────────────────────────────────────────

pub async fn run_tui(mut app: App) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the event loop; capture result so we always clean up
    let result = run_event_loop(&mut terminal, &mut app).await;

    // Always restore terminal
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();

    result
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    let tick_rate = Duration::from_millis(100);

    loop {
        // Drain any pending updates from the background task
        app.poll_updates();

        // Increment animation frame for UI effects
        app.animation_frame = app.animation_frame.wrapping_add(1);

        terminal.draw(|f| draw_ui(f, app))?;

        if app.should_quit {
            break;
        }

        if event::poll(tick_rate)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    match key.code {
                        // -- Approval keys --
                        KeyCode::Char('a')
                            if matches!(app.approval_state, ApprovalState::Pending) =>
                        {
                            app.approval_state = ApprovalState::Approved;
                            if let Err(e) = app.continue_after_approval().await {
                                app.is_processing = false;
                                app.messages
                                    .push(ChatMessage::error(&format!("Error: {}", e)));
                            }
                        }
                        KeyCode::Char('r')
                            if matches!(app.approval_state, ApprovalState::Pending) =>
                        {
                            app.approval_state = ApprovalState::Rejected;
                            if let Err(e) = app.continue_after_approval().await {
                                app.is_processing = false;
                                app.messages
                                    .push(ChatMessage::error(&format!("Error: {}", e)));
                            }
                        }

                        // -- Scroll --
                        KeyCode::PageUp => {
                            app.scroll_state.page_up();
                        }
                        KeyCode::PageDown => {
                            app.scroll_state.page_down();
                        }
                        KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.scroll_state.scroll_by(-3);
                        }
                        KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.scroll_state.scroll_by(3);
                        }
                        KeyCode::Home => {
                            if key.modifiers.contains(KeyModifiers::CONTROL) {
                                app.scroll_state.scroll_to_top();
                            } else {
                                // Scroll to top of viewport
                                app.scroll_state.scroll_to_top();
                            }
                        }
                        KeyCode::End => {
                            if key.modifiers.contains(KeyModifiers::CONTROL) {
                                app.scroll_state.scroll_to_bottom();
                            } else {
                                // Scroll to bottom
                                app.scroll_state.scroll_to_bottom();
                            }
                        }

                        // -- Selection --
                        KeyCode::Enter => {
                            if app.file_picker_active {
                                // Select file from picker
                                if let Some(file_path) = app.get_selected_file() {
                                    // Find position of last @ in input
                                    let before_cursor: String = app.input.chars().take(app.cursor_position).collect();
                                    if let Some(at_pos) = before_cursor.rfind('@') {
                                        // Remove from @ to cursor
                                        let new_input: String = app.input.chars().take(at_pos).collect();
                                        app.input = new_input;
                                        app.cursor_position = at_pos;

                                        // Insert selected file path
                                        app.input.insert_str(app.cursor_position, &file_path);
                                        app.cursor_position += file_path.len();
                                    }
                                    app.deactivate_file_picker();
                                }
                            } else if app.selection.is_active() {
                                // Copy selection
                                app.copy_selection();
                            } else if !app.is_processing
                                && !matches!(app.approval_state, ApprovalState::Pending)
                                && !app.input.trim().is_empty()
                            {
                                app.start_processing();
                            }
                        }
                        KeyCode::Esc => {
                            if app.file_picker_active {
                                // Cancel file picker
                                app.deactivate_file_picker();
                            } else if app.selection.is_active() {
                                app.clear_selection();
                            }
                        }

                        // -- Send (Ctrl+S) --
                        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if !app.is_processing
                                && !matches!(app.approval_state, ApprovalState::Pending)
                                && !app.input.trim().is_empty()
                                && !app.selection.is_active()
                            {
                                app.start_processing();
                            }
                        }

                        // -- Move to start (Ctrl+A) --
                        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.cursor_position = 0;
                        }

                        // -- Move to end (Ctrl+E) --
                        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.cursor_position = app.input.len();
                        }

                        // -- Copy (Ctrl+O) --
                        KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if app.selection.is_active() {
                                app.copy_selection();
                            } else {
                                // Copy all messages if no selection
                                let all_content = app
                                    .messages
                                    .iter()
                                    .filter(|m| &*m.role == "action")
                                    .map(|m| m.content.as_str())
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                if !all_content.is_empty() {
                                    let success = copy_to_clipboard(&all_content);
                                    if success {
                                        app.show_toast("Copied all content".to_string());
                                    } else {
                                        app.show_toast(
                                            "Copy failed - install xclip (Linux)".to_string(),
                                        );
                                    }
                                } else {
                                    app.show_toast("Nothing to copy".to_string());
                                }
                            }
                        }

                        // -- Paste (Ctrl+Y or Ctrl+Shift+V) --
                        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            match paste_from_clipboard() {
                                Some(content) => {
                                    app.input.insert_str(app.cursor_position, &content);
                                    app.cursor_position += content.len();
                                    app.show_toast(format!("Pasted {} chars", content.len()));
                                }
                                None => {
                                    app.show_toast(
                                        "Paste failed - install xclip/wl-clipboard".to_string(),
                                    );
                                }
                            }
                        }
                        KeyCode::Char('V')
                            if key.modifiers.contains(KeyModifiers::CONTROL)
                                && key.modifiers.contains(KeyModifiers::SHIFT) =>
                        {
                            match paste_from_clipboard() {
                                Some(content) => {
                                    app.input.insert_str(app.cursor_position, &content);
                                    app.cursor_position += content.len();
                                    app.show_toast(format!("Pasted {} chars", content.len()));
                                }
                                None => {
                                    app.show_toast(
                                        "Paste failed - install xclip/wl-clipboard".to_string(),
                                    );
                                }
                            }
                        }

                        // -- Ctrl+C: stop agent / double = quit --
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            let now = Instant::now();
                            if app.is_processing {
                                app.cancel_processing();
                                app.last_ctrl_c = Some(now);
                            } else if let Some(last) = app.last_ctrl_c {
                                if now.duration_since(last) < Duration::from_secs(2) {
                                    app.should_quit = true;
                                } else {
                                    app.last_ctrl_c = Some(now);
                                }
                            } else {
                                app.last_ctrl_c = Some(now);
                            }
                        }

                        // -- Text input --
                        KeyCode::Char(c) => {
                            if !app.is_processing
                                && !matches!(app.approval_state, ApprovalState::Pending)
                            {
                                // Exit history mode when typing
                                app.exit_history_mode();

                                // Check for @ symbol to activate file picker
                                if c == '@' && !app.file_picker_active {
                                    // Insert @ and activate picker
                                    let cursor = app.cursor_position;
                                    app.input.insert(cursor, c);
                                    app.cursor_position += 1;

                                    app.activate_file_picker(String::new());
                                    app.show_toast(format!("File picker: {} files", app.file_picker_results.len()));
                                } else if app.file_picker_active {
                                    // Update query and insert character
                                    let cursor = app.cursor_position;
                                    app.input.insert(cursor, c);
                                    app.cursor_position += 1;

                                    // Update picker query
                                    let before_at: String = app.input.chars().take(app.cursor_position).collect();
                                    let query_start = before_at.rfind('@').map(|i| i + 1).unwrap_or(0);
                                    let query: String = before_at.chars().skip(query_start).collect();

                                    if !query.is_empty() {
                                        app.activate_file_picker(query);
                                    } else {
                                        // Show all files when query is empty
                                        app.activate_file_picker(String::new());
                                    }
                                } else {
                                    let cursor = app.cursor_position;
                                    app.input.insert(cursor, c);
                                    app.cursor_position += 1;
                                }
                            }
                        }
                        KeyCode::Backspace => {
                            if !app.is_processing
                                && !matches!(app.approval_state, ApprovalState::Pending)
                                && app.cursor_position > 0
                            {
                                // Exit history mode when editing
                                app.exit_history_mode();

                                let cursor = app.cursor_position - 1;
                                app.input.remove(cursor);
                                app.cursor_position = cursor;

                                // Update file picker if active
                                if app.file_picker_active {
                                    let before_cursor: String = app.input.chars().take(app.cursor_position).collect();

                                    // Check if @ was deleted
                                    if !before_cursor.contains('@') {
                                        app.deactivate_file_picker();
                                    } else {
                                        // Update picker query
                                        let query_start = before_cursor.rfind('@').map(|i| i + 1).unwrap_or(0);
                                        let query: String = before_cursor.chars().skip(query_start).collect();

                                        if !query.is_empty() {
                                            app.activate_file_picker(query);
                                        } else {
                                            app.activate_file_picker(String::new());
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Up => {
                            // File picker navigation
                            if app.file_picker_active {
                                app.file_picker_prev();
                            } else {
                                // Navigate history
                                app.history_prev();
                            }
                        }
                        KeyCode::Down => {
                            // File picker navigation
                            if app.file_picker_active {
                                app.file_picker_next();
                            } else {
                                // Navigate history
                                app.history_next();
                            }
                        }
                        KeyCode::Left => {
                            if app.cursor_position > 0 {
                                app.cursor_position -= 1;
                            }
                        }
                        KeyCode::Right => {
                            if app.cursor_position < app.input.len() {
                                app.cursor_position += 1;
                            }
                        }
                        // -- Role cycling (Shift+Tab) --
                        KeyCode::BackTab => {
                            app.cycle_role();
                            app.show_toast(format!("Role: {}", app.role.name()));
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => {
                        app.scroll_state.scroll_by(3);
                    }
                    MouseEventKind::ScrollUp => {
                        app.scroll_state.scroll_by(-3);
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        app.handle_click(mouse.column, mouse.row);
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        app.handle_release();
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        app.handle_drag(mouse.column, mouse.row);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    Ok(())
}
// ── File Picker Popup ─────────────────────────────────────────────

fn draw_file_picker(f: &mut Frame, app: &App) {
    let results = &app.file_picker_results;

    if results.is_empty() {
        return;
    }

    // Calculate popup position and size
    let popup_width = 50u16;
    let max_height = 10u16;
    let total_height = results.len() as u16;
    let popup_height = total_height.min(max_height);

    // Calculate scroll offset to show selected item
    let scroll_offset = if app.file_picker_selected >= max_height as usize {
        app.file_picker_selected - (max_height as usize - 1)
    } else {
        0
    };

    // Position popup above input area
    let popup_area = ratatui::layout::Rect {
        x: 2,
        y: f.area().height.saturating_sub(popup_height + 3), // Above input (3 lines for padding)
        width: popup_width,
        height: popup_height,
    };

    // Get visible items (with scrolling)
    let visible_results: Vec<_> = results
        .iter()
        .skip(scroll_offset)
        .take(popup_height as usize)
        .enumerate()
        .collect();

    // Create popup items
    let items: Vec<Line> = visible_results
        .iter()
        .map(|(relative_i, file)| {
            let actual_index = scroll_offset + relative_i;
            let is_selected = actual_index == app.file_picker_selected;

            if is_selected {
                Line::from(vec![
                    Span::styled(
                        format!("> {}", file),
                        Style::default()
                            .fg(Color::Rgb(200, 200, 200))
                            .bg(Color::Rgb(60, 60, 80))
                            .add_modifier(Modifier::BOLD),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        format!("  {}", file),
                        Style::default().fg(Color::Rgb(180, 180, 190)),
                    ),
                ])
            }
        })
        .collect();

    let popup = Paragraph::new(items);

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(popup, popup_area);
}
