use crate::model::{Model, Message};
use crate::parser::{parse_action, Action};
use crate::error::AgentError;
use crate::tools::ToolRegistry;
use crate::tui_components::{
    ChatMessage, MarkdownRenderer, ApprovalState, PendingAction,
    render_message, render_hint_line, THEME,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind, MouseButton},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame, Terminal,
};
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use unicode_width::{UnicodeWidthStr, UnicodeWidthChar};

// For clipboard operations (click-to-copy)
fn copy_to_clipboard(text: &str) {
    use clipboard::ClipboardProvider;
    if let Ok(mut ctx) = clipboard::ClipboardContext::new() {
        let _ = ctx.set_contents(text.to_string());
    }
}

// ── Messages from background agent task to the UI ────────────

enum AgentUpdate {
    AssistantMessage(String),
    ActionMessage(String),
    OutputMessage(String),
    ErrorMessage(String),
    SystemMessage(String),
    UsageUpdate { tokens: u32, cost: f64 },
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
    yolo_mode: bool,
) {
    loop {
        // Check for cancellation
        if cancel_token.is_cancelled() {
            let _ = tx.send(AgentUpdate::SystemMessage("interrupted".to_string()));
            let _ = tx.send(AgentUpdate::Done);
            return;
        }

        let lm_output = match model.query(&conversation).await {
            Ok(output) => output,
            Err(e) => {
                let _ = tx.send(AgentUpdate::ErrorMessage(format!("Query error: {}", e)));
                let _ = tx.send(AgentUpdate::Done);
                return;
            }
        };

        // Check for cancellation after query
        if cancel_token.is_cancelled() {
            let _ = tx.send(AgentUpdate::SystemMessage("interrupted".to_string()));
            let _ = tx.send(AgentUpdate::Done);
            return;
        }

        // Send usage update if available
        if let Ok(usage) = model.get_usage() {
            let _ = tx.send(AgentUpdate::UsageUpdate {
                tokens: usage.completion_tokens.unwrap_or(0),
                cost: usage.cost_in_currency.unwrap_or(0.0),
            });
        }

        let _ = tx.send(AgentUpdate::AssistantMessage(lm_output.clone()));

        let action = match parse_action(&lm_output) {
            Ok(a) => a,
            Err(AgentError::Terminating(msg)) => {
                let _ = tx.send(AgentUpdate::ErrorMessage(msg));
                let _ = tx.send(AgentUpdate::Done);
                return;
            }
            Err(AgentError::Timeout(msg)) => {
                let _ = tx.send(AgentUpdate::ErrorMessage(msg.clone()));
                conversation.push(Message {
                    role: "user".to_string(),
                    content: msg,
                });
                continue;
            }
        };

        // Check for cancellation after parsing
        if cancel_token.is_cancelled() {
            let _ = tx.send(AgentUpdate::SystemMessage("interrupted".to_string()));
            let _ = tx.send(AgentUpdate::Done);
            return;
        }

        let action_display = match &action {
            Action::Bash { command, workdir, timeout_ms, env } => {
                let mut desc = format!("bash: {}", command);
                if let Some(dir) = workdir {
                    desc.push_str(&format!(" (workdir: {})", dir));
                }
                if let Some(timeout) = timeout_ms {
                    desc.push_str(&format!(" (timeout: {}ms)", timeout));
                }
                if env.as_ref().map(|e| !e.is_null()).unwrap_or(false) {
                    desc.push_str(" (env vars set)");
                }
                desc
            }
            Action::Tool { name, params } => format!("tool: {} | {}", name, params),
            Action::Response(_) => {
                // Plain text response - mark as complete
                let _ = tx.send(AgentUpdate::Done);
                return;
            }
        };
        let _ = tx.send(AgentUpdate::ActionMessage(action_display));

        match &action {
            Action::Bash { command, .. } => {
                if yolo_mode {
                    // YOLO mode: execute directly
                    match execute_bash_action(command) {
                        Ok(output) => {
                            let _ = tx.send(AgentUpdate::OutputMessage(output.clone()));
                            conversation.push(Message { role: "assistant".to_string(), content: lm_output });
                            conversation.push(Message { role: "user".to_string(), content: output });
                        }
                        Err(e) => {
                            let _ = tx.send(AgentUpdate::ErrorMessage(format!("Error: {}", e)));
                            let _ = tx.send(AgentUpdate::Done);
                            return;
                        }
                    }
                } else {
                    // Normal mode: signal that approval is needed
                    // We'll handle approval in the main event loop
                    let _ = tx.send(AgentUpdate::Done);
                    return;
                }
            }
            Action::Tool { name, params } => {
                match tool_registry.execute(name, params.clone()) {
                    Ok(output) => {
                        // Special handling for read tool: show simplified message
                        let display_message = if name == "read" {
                            // Try to extract file_path from params for a nicer message
                            if let Ok(params_obj) = serde_json::from_value::<serde_json::Value>(params.clone()) {
                                if let Some(file_path) = params_obj.get("file_path").and_then(|v| v.as_str()) {
                                    format!("Reading {}", file_path)
                                } else {
                                    "Reading file...".to_string()
                                }
                            } else {
                                "Reading file...".to_string()
                            }
                        } else {
                            let mut result = output.content.clone();
                            if !output.attachments.is_empty() {
                                result.push_str(&format!("\n[{} attachment(s)]", output.attachments.len()));
                            }
                            if let Some(line_count) = output.metadata.line_count {
                                result.push_str(&format!("\n[Lines: {}]", line_count));
                            }
                            if output.metadata.truncated {
                                result.push_str(" [truncated]");
                            }
                            result
                        };

                        let _ = tx.send(AgentUpdate::OutputMessage(display_message));
                        // Send the full result to the conversation for model context
                        conversation.push(Message { role: "assistant".to_string(), content: lm_output });
                        conversation.push(Message { role: "user".to_string(), content: output.content });
                    }
                    Err(e) => {
                        let _ = tx.send(AgentUpdate::OutputMessage(format!("Tool error: {}", e)));
                        conversation.push(Message { role: "assistant".to_string(), content: lm_output });
                        conversation.push(Message { role: "user".to_string(), content: format!("Tool error: {}", e) });
                    }
                }
            }
            Action::Response(response) => {
                // Plain text response - no tools needed
                // Add to conversation (will be displayed in UI)
                conversation.push(Message { role: "assistant".to_string(), content: lm_output });
                conversation.push(Message { role: "user".to_string(), content: response.clone() });
                // Don't send OutputMessage - it's already in the conversation display
            }
        }
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
    #[allow(dead_code)]
    pub markdown_renderer: MarkdownRenderer,
    pub pending_action: Option<PendingAction>,
    pub approval_state: ApprovalState,
    pub streaming_content: String,
    pub session_tokens: u32,
    pub session_cost: f64,
    pub model_name: String,
    pub scroll_offset: usize,
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
}

impl App {
    pub fn new(model: Model, tool_registry: ToolRegistry, model_name: String, yolo_mode: bool) -> Self {
        let tools_description = tool_registry.descriptions();
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
            markdown_renderer: MarkdownRenderer::new(),
            pending_action: None,
            approval_state: ApprovalState::None,
            streaming_content: String::new(),
            session_tokens: 0,
            session_cost: 0.0,
            model_name,
            scroll_offset: 0,
            last_ctrl_c: None,
            yolo_mode,
            update_rx: None,
            cancel_token: CancellationToken::new(),
            rendered_lines: Vec::new(),
            conversation_area: None,
        }
    }

    pub fn set_system_prompt(&mut self, system_prompt: &str) {
        self.system_messages = vec![Message {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        }];
    }

    fn get_conversation_history(&self) -> Vec<Message> {
        let mut messages = self.system_messages.clone();
        for chat_msg in &self.messages {
            match chat_msg.role.as_str() {
                "user" | "assistant" => {
                    messages.push(Message {
                        role: chat_msg.role.clone(),
                        content: chat_msg.content.clone(),
                    });
                }
                _ => {}
            }
        }
        messages
    }

    /// Start processing a message in the background (non-blocking).
    pub fn start_processing(&mut self) {
        if self.input.trim().is_empty() {
            return;
        }

        let user_input = self.input.clone();
        self.messages.push(ChatMessage::user(&user_input));
        self.input.clear();
        self.cursor_position = 0;
        self.is_processing = true;
        self.streaming_content.clear();
        self.scroll_offset = 0;

        let conversation = self.get_conversation_history();
        let model = self.model.clone();
        let tool_registry = Arc::clone(&self.tool_registry);
        let tools_description = self.tools_description.clone();
        let yolo_mode = self.yolo_mode;

        let (tx, rx) = mpsc::unbounded_channel();
        self.update_rx = Some(rx);

        let cancel_token = CancellationToken::new();
        self.cancel_token = cancel_token.clone();

        tokio::spawn(async move {
            agent_loop(model, tool_registry, tools_description, conversation, tx, cancel_token, yolo_mode).await;
        });
    }

    /// Drain pending updates from the background task channel.
    fn poll_updates(&mut self) {
        let rx = match &mut self.update_rx {
            Some(rx) => rx,
            None => return,
        };

        loop {
            match rx.try_recv() {
                Ok(update) => match update {
                    AgentUpdate::AssistantMessage(content) => {
                        self.messages.push(ChatMessage::assistant_with_thinking(&content));
                        self.scroll_offset = 0;
                    }
                    AgentUpdate::ActionMessage(content) => {
                        self.messages.push(ChatMessage::action(&content));
                        self.scroll_offset = 0;
                        // Check if this is a bash action that needs approval (not in YOLO mode)
                        if !self.yolo_mode && content.starts_with("bash:") {
                            // Extract command from the action display
                            let command = content.strip_prefix("bash: ").unwrap_or(&content);
                            self.pending_action = Some(PendingAction {
                                action_type: "bash".to_string(),
                                content: command.to_string(),
                            });
                            self.approval_state = ApprovalState::Pending;
                            self.is_processing = false;
                            // Cancel the background task
                            self.cancel_token.cancel();
                            self.update_rx = None;
                            return;
                        }
                    }
                    AgentUpdate::OutputMessage(content) => {
                        self.messages.push(ChatMessage::output(&content));
                        self.scroll_offset = 0;
                    }
                    AgentUpdate::ErrorMessage(content) => {
                        self.messages.push(ChatMessage::error(&content));
                        self.scroll_offset = 0;
                    }
                    AgentUpdate::SystemMessage(content) => {
                        self.messages.push(ChatMessage::system(&content));
                        self.scroll_offset = 0;
                    }
                    AgentUpdate::UsageUpdate { tokens, cost } => {
                        self.session_tokens += tokens;
                        self.session_cost += cost;
                    }
                    AgentUpdate::Done => {
                        self.is_processing = false;
                        self.streaming_content.clear();
                        self.update_rx = None;
                        return;
                    }
                },
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.is_processing = false;
                    self.streaming_content.clear();
                    self.update_rx = None;
                    return;
                }
            }
        }
    }

    /// Cancel the current processing.
    pub fn cancel_processing(&mut self) {
        self.cancel_token.cancel();
        self.is_processing = false;
        self.streaming_content.clear();
    }

    /// Handle a mouse click in the conversation area and copy the clicked line to clipboard
    pub fn handle_click(&mut self, column: u16, row: u16) {
        let Some(area) = self.conversation_area else {
            return;
        };

        // Check if click is within conversation area bounds
        if column < area.left() || column >= area.right() || row < area.top() || row >= area.bottom() {
            return;
        }

        // Calculate visible height (accounting for borders)
        let visible_height = area.height.saturating_sub(2) as usize;

        // Calculate total lines and scroll info
        let total_lines = self.rendered_lines.len();
        let max_scroll = total_lines.saturating_sub(visible_height);
        let scroll = self.scroll_offset.min(max_scroll);
        let bottom_scroll = max_scroll.saturating_sub(scroll);

        // Calculate which line was clicked (relative to top of visible area)
        let click_row_in_area = (row - area.top()) as usize;

        // Calculate the actual line index in the rendered lines
        // bottom_scroll is the line index that appears at the top
        let line_index = bottom_scroll.saturating_sub(click_row_in_area);

        if line_index < self.rendered_lines.len() {
            let text = &self.rendered_lines[line_index];

            // Trim leading whitespace for cleaner copy
            let text_to_copy = text.trim();

            if !text_to_copy.is_empty() {
                copy_to_clipboard(text_to_copy);
            }
        }
    }

    pub async fn continue_after_approval(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !matches!(self.approval_state, ApprovalState::Pending) {
            return Ok(());
        }

        let pending = self.pending_action.take();
        let approval = self.approval_state.clone();
        self.approval_state = ApprovalState::None;

        let mut conversation = self.get_conversation_history();
        
        if let Some(last_msg) = self.messages.last()
            && last_msg.role == "action" {
                conversation.push(Message {
                    role: "assistant".to_string(),
                    content: last_msg.content.clone(),
                });
            }

        match approval {
            ApprovalState::Approved => {
                if let Some(PendingAction { content: cmd, .. }) = pending {
                    let output = execute_bash_action(&cmd)?;
                    self.messages.push(ChatMessage::output(&output));
                    conversation.push(Message { role: "user".to_string(), content: output });
                }
            }
            ApprovalState::Rejected => {
                self.messages.push(ChatMessage::output("Action rejected by user"));
                conversation.push(Message { role: "user".to_string(), content: "Action rejected".to_string() });
            }
            _ => {}
        }

        // Restart the agent loop in background
        self.is_processing = true;
        let model = self.model.clone();
        let tool_registry = Arc::clone(&self.tool_registry);
        let tools_description = self.tools_description.clone();
        let yolo_mode = self.yolo_mode;

        let (tx, rx) = mpsc::unbounded_channel();
        self.update_rx = Some(rx);

        let cancel_token = CancellationToken::new();
        self.cancel_token = cancel_token.clone();

        tokio::spawn(async move {
            agent_loop(model, tool_registry, tools_description, conversation, tx, cancel_token, yolo_mode).await;
        });

        Ok(())
    }
}

fn execute_bash_action(command: &str) -> Result<String, AgentError> {
    use std::process::Command;

    if command.trim() == "exit" {
        return Err(AgentError::Terminating("Agent requested to exit".to_string()));
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
        Err(e) => Err(AgentError::Timeout(format!("Command execution failed: {}", e))),
    }
}

// ── UI Drawing ───────────────────────────────────────────────

fn draw_ui(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // Three-zone vertical layout: header | conversation | input area
    let zones = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // header bar
            Constraint::Length(1),  // spacer
            Constraint::Min(1),    // conversation
            Constraint::Length(3), // input + hint
        ])
        .split(area);

    // ── Header bar ───────────────────────────────────────────
    draw_header(f, app, zones[0]);

    // ── Conversation area ────────────────────────────────────
    draw_conversation(f, app, zones[2]);

    // ── Input area ───────────────────────────────────────────
    draw_input(f, app, zones[3]);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let t = &*THEME;
    let width = area.width as usize;

    let left_text = "  codr";
    let right_text = format!("{}  {}tok  ${:.4}  ", app.model_name, app.session_tokens, app.session_cost);
    let padding = width.saturating_sub(left_text.len() + right_text.len());

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  codr", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(" ".repeat(padding), t.dim),
        Span::styled(right_text, t.status),
    ]));

    f.render_widget(header, area);
}

fn draw_conversation(f: &mut Frame, app: &mut App, area: Rect) {
    let t = &*THEME;
    let width = area.width.saturating_sub(2) as usize; // account for borders

    // Store conversation area for click handling
    app.conversation_area = Some(area);

    // Render all messages into lines
    let mut all_lines: Vec<Line<'static>> = Vec::new();
    let mut rendered_text: Vec<String> = Vec::new(); // Store raw text for copying

    // Welcome message if no messages yet
    if app.messages.is_empty() {
        all_lines.push(Line::from(""));
        let welcome_text = "Welcome to codr. Type a message to get started.";
        rendered_text.push(String::new());
        all_lines.push(Line::from(vec![
            Span::styled("  Welcome to ", t.dim),
            Span::styled("codr", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(". Type a message to get started.", t.dim),
        ]));
        rendered_text.push(welcome_text.to_string());
        all_lines.push(Line::from(""));
        rendered_text.push(String::new());
    }

    for msg in &app.messages {
        let rendered = render_message(msg, width);
        for line in &rendered {
            // Extract raw text from spans for copying
            let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
            rendered_text.push(text);
        }
        all_lines.extend(rendered);
    }

    // Store rendered text for click handling
    app.rendered_lines = rendered_text;

    let visible_height = area.height.saturating_sub(2) as usize; // borders
    let total_lines = all_lines.len();

    // Clamp scroll offset
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = app.scroll_offset.min(max_scroll);

    // Scroll from bottom: when scroll_offset == 0, show the last lines
    let bottom_scroll = max_scroll.saturating_sub(scroll);

    let conversation = Paragraph::new(all_lines)
        .scroll((bottom_scroll as u16, 0));

    f.render_widget(conversation, area);
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let t = &*THEME;
    let is_pending = matches!(app.approval_state, ApprovalState::Pending);

    // Input area: input (2) + hint (1) = 3 total
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // input area (2 lines)
            Constraint::Length(1), // hint line
        ])
        .split(area);

    // Input style
    let input_style = if app.is_processing {
        Style::default().fg(Color::DarkGray)
    } else if is_pending {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };

    // Prompt prefix - Claude Code style: model name + indicator
    let (prompt_label, prompt_indicator) = if is_pending {
        ("approve / reject".to_string(), "")
    } else if app.is_processing {
        (app.model_name.clone(), " ◉")
    } else {
        (app.model_name.clone(), " >")
    };

    // Build input text with prompt
    let prompt_style = if is_pending {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else if app.is_processing {
        Style::default().fg(Color::Rgb(100, 100, 120))
    } else {
        t.prompt
    };

    // Prompt span for reuse
    let prompt_span = Span::styled(format!("{}{} ", prompt_label, prompt_indicator), prompt_style);

    // Context tokens display on the right
    let tokens_info = if !app.is_processing && !is_pending {
        format!("{}t ", app.session_tokens)
    } else {
        String::new()
    };
    let tokens_info_width = tokens_info.len();

    // Wrap input to fit width (accounting for prompt prefix and tokens on right)
    let available_width = (chunks[0].width as usize).saturating_sub(tokens_info_width);
    let prefix_width = prompt_label.width() + prompt_indicator.width() + 1;

    // Simple wrapping for display
    let input_text = app.input.as_str();
    let mut display_lines = Vec::new();

    // First line: prompt + first part of input
    let first_line_capacity = available_width.saturating_sub(prefix_width);
    let (first_line, remaining) = if input_text.width() > first_line_capacity {
        // Truncate for first line
        let truncated = truncate_text(input_text, first_line_capacity);
        (truncated, Some(&input_text[truncated.len()..]))
    } else {
        (input_text, None)
    };

    // Calculate padding to keep tokens fixed on right
    let text_width = first_line.width();
    let padding_width = first_line_capacity.saturating_sub(text_width);
    let padding = " ".repeat(padding_width);

    display_lines.push(Line::from(vec![
        prompt_span.clone(),
        Span::styled(first_line, input_style),
        Span::styled(&padding, Style::default()),
        Span::styled(&tokens_info, t.dim),
    ]));

    // Second line (if there's remaining text or always show second line for spacing)
    if let Some(remaining_text) = remaining {
        let truncated = truncate_text(remaining_text, available_width);
        display_lines.push(Line::from(vec![
            Span::styled(format!("{} ", " ".repeat(prefix_width)), Style::default()),
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
        let cursor_offset = app.input.width().min(first_line_capacity);
        if cursor_offset < first_line_capacity {
            // Cursor on first line
            f.set_cursor_position((
                chunks[0].x + prefix_width as u16 + cursor_offset as u16,
                chunks[0].y,
            ));
        } else {
            // Cursor on second line
            let remaining_offset = app.input.width().saturating_sub(first_line_capacity);
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

    // Hint line
    let hint = Paragraph::new(render_hint_line(is_pending));
    f.render_widget(hint, chunks[1]);
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
                        KeyCode::Char('a') if matches!(app.approval_state, ApprovalState::Pending) => {
                            app.approval_state = ApprovalState::Approved;
                            if let Err(e) = app.continue_after_approval().await {
                                app.is_processing = false;
                                app.messages.push(ChatMessage::error(&format!("Error: {}", e)));
                            }
                        }
                        KeyCode::Char('r') if matches!(app.approval_state, ApprovalState::Pending) => {
                            app.approval_state = ApprovalState::Rejected;
                            if let Err(e) = app.continue_after_approval().await {
                                app.is_processing = false;
                                app.messages.push(ChatMessage::error(&format!("Error: {}", e)));
                            }
                        }

                        // -- Scroll --
                        KeyCode::PageUp => {
                            app.scroll_offset = app.scroll_offset.saturating_add(10);
                        }
                        KeyCode::PageDown => {
                            app.scroll_offset = app.scroll_offset.saturating_sub(10);
                        }
                        KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.scroll_offset = app.scroll_offset.saturating_add(3);
                        }
                        KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.scroll_offset = app.scroll_offset.saturating_sub(3);
                        }

                        // -- Send (Ctrl+S or Enter) --
                        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if !app.is_processing
                                && !matches!(app.approval_state, ApprovalState::Pending)
                                && !app.input.trim().is_empty()
                            {
                                app.start_processing();
                            }
                        }
                        KeyCode::Enter => {
                            if !app.is_processing
                                && !matches!(app.approval_state, ApprovalState::Pending)
                                && !app.input.trim().is_empty()
                            {
                                app.start_processing();
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
                            if !app.is_processing && !matches!(app.approval_state, ApprovalState::Pending) {
                                let cursor = app.cursor_position;
                                app.input.insert(cursor, c);
                                app.cursor_position += 1;
                            }
                        }
                        KeyCode::Backspace => {
                            if !app.is_processing && !matches!(app.approval_state, ApprovalState::Pending)
                                && app.cursor_position > 0 {
                                    let cursor = app.cursor_position - 1;
                                    app.input.remove(cursor);
                                    app.cursor_position = cursor;
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
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => {
                        app.scroll_offset = app.scroll_offset.saturating_add(3);
                    }
                    MouseEventKind::ScrollUp => {
                        app.scroll_offset = app.scroll_offset.saturating_sub(3);
                    }
                    MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                        app.handle_click(mouse.column, mouse.row);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    Ok(())
}
