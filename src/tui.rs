use crate::model::{Model, Message};
use crate::parser::{parse_action, Action, format_available_tools};
use crate::error::AgentError;
use crate::tools::ToolRegistry;
use crate::tui_components::{
    ChatMessage, MarkdownRenderer, ApprovalState, PendingAction,
    render_message, render_hint_line, THEME,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

pub struct App {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub cursor_position: usize,
    pub should_quit: bool,
    pub is_processing: bool,
    pub model: Model,
    pub tool_registry: ToolRegistry,
    pub tools_description: String,
    pub system_messages: Vec<Message>,
    pub markdown_renderer: MarkdownRenderer,
    pub pending_action: Option<PendingAction>,
    pub approval_state: ApprovalState,
    pub streaming_content: String,
    pub session_tokens: u32,
    pub session_cost: f64,
    pub model_name: String,
    pub scroll_offset: u16,
}

impl App {
    pub fn new(model: Model, tool_registry: ToolRegistry, model_name: String) -> Self {
        let tools_description = tool_registry.descriptions();
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor_position: 0,
            should_quit: false,
            is_processing: false,
            model,
            tool_registry,
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

    /// Scroll to the bottom of the chat (reset offset)
    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub async fn process_message(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.input.trim().is_empty() {
            return Ok(());
        }

        let user_input = self.input.clone();
        self.messages.push(ChatMessage::user(&user_input));
        self.input.clear();
        self.cursor_position = 0;
        self.is_processing = true;
        self.streaming_content.clear();
        self.scroll_to_bottom();

        let mut conversation = self.get_conversation_history();

        loop {
            let lm_output = self.model.query(&conversation).await?;

            if let Ok(usage) = self.model.get_usage() {
                self.session_tokens += usage.completion_tokens.unwrap_or(0);
                self.session_cost += usage.cost_in_currency.unwrap_or(0.0);
            }

            self.messages.push(ChatMessage::assistant(&lm_output));
            self.scroll_to_bottom();

            let action = match parse_action(&lm_output) {
                Ok(a) => a,
                Err(AgentError::FormatError(msg)) => {
                    let enhanced_msg = format!("{}\n\n{}", msg, format_available_tools(&self.tools_description));
                    self.messages.push(ChatMessage::error(&enhanced_msg));
                    conversation.push(Message {
                        role: "user".to_string(),
                        content: enhanced_msg,
                    });
                    continue;
                }
                Err(AgentError::TerminatingError(msg)) => {
                    self.messages.push(ChatMessage::error(&msg));
                    break;
                }
                Err(AgentError::TimeoutError(msg)) => {
                    self.messages.push(ChatMessage::error(&msg));
                    conversation.push(Message {
                        role: "user".to_string(),
                        content: msg,
                    });
                    continue;
                }
            };

            let action_display = match &action {
                Action::Bash(cmd) => format!("bash: {}", cmd),
                Action::Tool { name, params } => format!("tool: {} | {}", name, params),
            };
            self.messages.push(ChatMessage::action(&action_display));
            self.scroll_to_bottom();

            match &action {
                Action::Bash(cmd) => {
                    self.pending_action = Some(PendingAction {
                        action_type: "bash".to_string(),
                        content: cmd.clone(),
                    });
                    self.approval_state = ApprovalState::Pending;
                    return Ok(());
                }
                Action::Tool { name, params } => {
                    match self.tool_registry.execute(name, params.clone()) {
                        Ok(output) => {
                            let mut result = output.content;
                            if !output.attachments.is_empty() {
                                result.push_str(&format!("\n[{} attachment(s)]", output.attachments.len()));
                            }
                            if let Some(line_count) = output.metadata.line_count {
                                result.push_str(&format!("\n[Lines: {}]", line_count));
                            }
                            if output.metadata.truncated {
                                result.push_str(" [truncated]");
                            }
                            self.messages.push(ChatMessage::output(&result));
                            self.scroll_to_bottom();
                            conversation.push(Message { role: "assistant".to_string(), content: lm_output });
                            conversation.push(Message { role: "user".to_string(), content: result });
                        }
                        Err(e) => {
                            self.messages.push(ChatMessage::output(&format!("Tool error: {}", e)));
                            self.scroll_to_bottom();
                            conversation.push(Message { role: "assistant".to_string(), content: lm_output });
                            conversation.push(Message { role: "user".to_string(), content: format!("Tool error: {}", e) });
                        }
                    }
                }
            }

            self.pending_action = None;
            self.approval_state = ApprovalState::None;
        }

        self.is_processing = false;
        self.streaming_content.clear();
        Ok(())
    }

    pub async fn continue_after_approval(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !matches!(self.approval_state, ApprovalState::Pending) {
            return Ok(());
        }

        let pending = self.pending_action.take();
        let approval = self.approval_state.clone();
        self.approval_state = ApprovalState::None;

        let mut conversation = self.get_conversation_history();

        if let Some(last_msg) = self.messages.last() {
            if last_msg.role == "action" {
                conversation.push(Message {
                    role: "assistant".to_string(),
                    content: last_msg.content.clone(),
                });
            }
        }

        match approval {
            ApprovalState::Approved => {
                if let Some(PendingAction { content: cmd, .. }) = pending {
                    let output = execute_bash_action(&cmd)?;
                    self.messages.push(ChatMessage::output(&output));
                    self.scroll_to_bottom();
                    conversation.push(Message { role: "user".to_string(), content: output });
                }
            }
            ApprovalState::Rejected => {
                self.messages.push(ChatMessage::output("Action rejected by user"));
                self.scroll_to_bottom();
                conversation.push(Message { role: "user".to_string(), content: "Action rejected".to_string() });
            }
            _ => {}
        }

        loop {
            let lm_output = self.model.query(&conversation).await?;

            if let Ok(usage) = self.model.get_usage() {
                self.session_tokens += usage.completion_tokens.unwrap_or(0);
                self.session_cost += usage.cost_in_currency.unwrap_or(0.0);
            }

            self.messages.push(ChatMessage::assistant(&lm_output));
            self.scroll_to_bottom();

            let action = match parse_action(&lm_output) {
                Ok(a) => a,
                Err(AgentError::FormatError(msg)) => {
                    let enhanced_msg = format!("{}\n\n{}", msg, format_available_tools(&self.tools_description));
                    self.messages.push(ChatMessage::error(&enhanced_msg));
                    conversation.push(Message {
                        role: "user".to_string(),
                        content: enhanced_msg,
                    });
                    continue;
                }
                Err(AgentError::TerminatingError(msg)) => {
                    self.messages.push(ChatMessage::error(&msg));
                    break;
                }
                Err(AgentError::TimeoutError(msg)) => {
                    self.messages.push(ChatMessage::error(&msg));
                    conversation.push(Message {
                        role: "user".to_string(),
                        content: msg,
                    });
                    continue;
                }
            };

            let action_display = match &action {
                Action::Bash(cmd) => format!("bash: {}", cmd),
                Action::Tool { name, params } => format!("tool: {} | {}", name, params),
            };
            self.messages.push(ChatMessage::action(&action_display));
            self.scroll_to_bottom();

            match &action {
                Action::Bash(cmd) => {
                    self.pending_action = Some(PendingAction {
                        action_type: "bash".to_string(),
                        content: cmd.clone(),
                    });
                    self.approval_state = ApprovalState::Pending;
                    return Ok(());
                }
                Action::Tool { name, params } => {
                    match self.tool_registry.execute(name, params.clone()) {
                        Ok(output) => {
                            let mut result = output.content;
                            if !output.attachments.is_empty() {
                                result.push_str(&format!("\n[{} attachment(s)]", output.attachments.len()));
                            }
                            if let Some(line_count) = output.metadata.line_count {
                                result.push_str(&format!("\n[Lines: {}]", line_count));
                            }
                            if output.metadata.truncated {
                                result.push_str(" [truncated]");
                            }
                            self.messages.push(ChatMessage::output(&result));
                            self.scroll_to_bottom();
                            conversation.push(Message { role: "assistant".to_string(), content: lm_output });
                            conversation.push(Message { role: "user".to_string(), content: result });
                        }
                        Err(e) => {
                            self.messages.push(ChatMessage::output(&format!("Tool error: {}", e)));
                            self.scroll_to_bottom();
                            conversation.push(Message { role: "assistant".to_string(), content: lm_output });
                            conversation.push(Message { role: "user".to_string(), content: format!("Tool error: {}", e) });
                        }
                    }
                }
            }

            self.pending_action = None;
            self.approval_state = ApprovalState::None;
        }

        Ok(())
    }
}

// ── Bash execution ───────────────────────────────────────────

fn execute_bash_action(command: &str) -> Result<String, AgentError> {
    use std::process::Command;

    if command.trim() == "exit" {
        return Err(AgentError::TerminatingError("Agent requested to exit".to_string()));
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
        Err(e) => Err(AgentError::TimeoutError(format!("Command execution failed: {}", e))),
    }
}

// ── Drawing ──────────────────────────────────────────────────

fn draw_ui(f: &mut Frame, app: &App) {
    let area = f.area();
    let t = &*THEME;

    // Layout: header (1) | messages (flex) | separator (1) | status (1) | hint (1) | input (1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // header
            Constraint::Min(1),    // messages
            Constraint::Length(1), // separator
            Constraint::Length(1), // status
            Constraint::Length(1), // hint line
            Constraint::Length(1), // input
        ])
        .split(area);

    draw_header(f, app, chunks[0]);
    draw_messages(f, app, chunks[1]);
    draw_separator(f, chunks[2]);
    draw_status(f, app, chunks[3]);
    draw_hints(f, app, chunks[4]);
    draw_input(f, app, chunks[5]);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let t = &*THEME;
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let header = Line::from(vec![
        Span::styled("  codr", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {}  ", app.model_name), t.dim),
        Span::styled(cwd, Style::default().fg(Color::Rgb(80, 80, 100))),
    ]);

    f.render_widget(Paragraph::new(header), area);
}

fn draw_messages(f: &mut Frame, app: &App, area: Rect) {
    let width = area.width as usize;

    // Build all display lines from messages
    let mut all_lines: Vec<Line<'static>> = Vec::new();

    if app.messages.is_empty() {
        // Welcome message
        all_lines.push(Line::from(""));
        all_lines.push(Line::from(vec![
            Span::styled("  Welcome to ", Style::default().fg(Color::DarkGray)),
            Span::styled("codr", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(". Type a message below to get started.", Style::default().fg(Color::DarkGray)),
        ]));
        all_lines.push(Line::from(""));
    } else {
        for msg in &app.messages {
            let rendered = render_message(msg, width);
            all_lines.extend(rendered);
        }
    }

    // Show spinner if processing
    if app.is_processing {
        all_lines.push(Line::from(vec![
            Span::styled("  ● ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("thinking...", Style::default().fg(Color::DarkGray)),
        ]));
    }

    let total_lines = all_lines.len() as u16;
    let visible_height = area.height;

    // Calculate scroll: we show the bottom of the chat by default (scroll_offset=0 means bottom)
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = max_scroll.saturating_sub(app.scroll_offset);

    let paragraph = Paragraph::new(all_lines)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn draw_separator(f: &mut Frame, area: Rect) {
    let t = &*THEME;
    let sep = "─".repeat(area.width as usize);
    let line = Paragraph::new(Line::from(Span::styled(sep, t.separator)));
    f.render_widget(line, area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let t = &*THEME;
    let tokens_str = if app.session_tokens > 0 {
        format!("  {}tok", app.session_tokens)
    } else {
        String::new()
    };
    let cost_str = if app.session_cost > 0.0 {
        format!("  ${:.4}", app.session_cost)
    } else {
        String::new()
    };

    let status = Line::from(vec![
        Span::styled(format!("  {}", app.model_name), t.status),
        Span::styled(tokens_str, t.dim),
        Span::styled(cost_str, t.dim),
    ]);

    f.render_widget(Paragraph::new(status), area);
}

fn draw_hints(f: &mut Frame, app: &App, area: Rect) {
    let hint = render_hint_line(matches!(app.approval_state, ApprovalState::Pending));
    f.render_widget(Paragraph::new(hint), area);
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let t = &*THEME;

    let input_style = if app.is_processing {
        t.dim
    } else if matches!(app.approval_state, ApprovalState::Pending) {
        THEME.action
    } else {
        Style::default().fg(Color::White)
    };

    let prompt_char = if matches!(app.approval_state, ApprovalState::Pending) {
        "? "
    } else {
        "> "
    };

    let input_line = Line::from(vec![
        Span::styled(format!("  {}", prompt_char), t.prompt),
        Span::styled(app.input.as_str(), input_style),
    ]);

    f.render_widget(Paragraph::new(input_line), area);

    // Position cursor after the prompt prefix ("  > " = 4 chars) plus input text
    let cursor_x = area.x + 4 + app.input[..app.cursor_position].width() as u16;
    let cursor_x = cursor_x.min(area.x + area.width.saturating_sub(1));
    f.set_cursor_position((cursor_x, area.y));
}

// ── Main event loop ──────────────────────────────────────────

pub async fn run_tui(app: App) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app_arc = Arc::new(Mutex::new(app));
    let app_clone = app_arc.clone();

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let app = app_clone.lock().unwrap();
            if app.should_quit {
                break;
            }
        }
    });

    let tick_rate = Duration::from_millis(100);

    loop {
        let app = app_arc.lock().unwrap();

        terminal.draw(|f| draw_ui(f, &app))?;

        if app.should_quit {
            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )?;
            terminal.show_cursor()?;
            break;
        }

        drop(app);

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                let mut app = app_arc.lock().unwrap();

                match key.code {
                    // ── Approval keys ────────────────────
                    KeyCode::Char('a') if matches!(app.approval_state, ApprovalState::Pending) => {
                        let mut app = app_arc.lock().unwrap();
                        app.approval_state = ApprovalState::Approved;
                        drop(app);
                        let mut app = app_arc.lock().unwrap();
                        app.continue_after_approval().await?;
                    }
                    KeyCode::Char('r') if matches!(app.approval_state, ApprovalState::Pending) => {
                        let mut app = app_arc.lock().unwrap();
                        app.approval_state = ApprovalState::Rejected;
                        drop(app);
                        let mut app = app_arc.lock().unwrap();
                        app.continue_after_approval().await?;
                    }

                    // ── Quit ──────────────────────────────
                    KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let mut app = app_arc.lock().unwrap();
                        app.should_quit = true;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let mut app = app_arc.lock().unwrap();
                        app.should_quit = true;
                    }

                    // ── Scroll ────────────────────────────
                    KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        app.scroll_offset = app.scroll_offset.saturating_add(3);
                    }
                    KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        app.scroll_offset = app.scroll_offset.saturating_sub(3);
                    }
                    KeyCode::PageUp => {
                        app.scroll_offset = app.scroll_offset.saturating_add(10);
                    }
                    KeyCode::PageDown => {
                        app.scroll_offset = app.scroll_offset.saturating_sub(10);
                    }

                    // ── Send message (Enter) ─────────────
                    KeyCode::Enter => {
                        if !app.is_processing && !matches!(app.approval_state, ApprovalState::Pending) {
                            let has_input = !app.input.trim().is_empty();
                            drop(app);
                            if has_input {
                                let mut app = app_arc.lock().unwrap();
                                app.process_message().await?;
                            }
                        }
                    }

                    // ── Text input ───────────────────────
                    KeyCode::Char(c) => {
                        if !app.is_processing && !matches!(app.approval_state, ApprovalState::Pending) {
                            let cursor = app.cursor_position;
                            app.input.insert(cursor, c);
                            app.cursor_position += 1;
                        }
                    }
                    KeyCode::Backspace => {
                        if !app.is_processing && !matches!(app.approval_state, ApprovalState::Pending) {
                            if app.cursor_position > 0 {
                                let cursor = app.cursor_position - 1;
                                app.input.remove(cursor);
                                app.cursor_position = cursor;
                            }
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
        }
    }

    Ok(())
}
