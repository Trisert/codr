use crate::model::{Model, Message};
use crate::parser::{parse_action, Action, format_available_tools};
use crate::error::AgentError;
use crate::tools::ToolRegistry;
use crate::tui_components::{
    ChatMessage, MarkdownRenderer, ApprovalState, PendingAction,
    render_message, render_hint_line, THEME,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::io;
use std::time::{Duration, Instant};
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
            last_ctrl_c: None,
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
        // Auto-scroll to bottom on new message
        self.scroll_offset = 0;

        let mut conversation = self.get_conversation_history();

        loop {
            let lm_output = self.model.query(&conversation).await?;
            
            if let Ok(usage) = self.model.get_usage() {
                self.session_tokens += usage.completion_tokens.unwrap_or(0);
                self.session_cost += usage.cost_in_currency.unwrap_or(0.0);
            }

            self.messages.push(ChatMessage::assistant(&lm_output));

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
                            conversation.push(Message { role: "assistant".to_string(), content: lm_output });
                            conversation.push(Message { role: "user".to_string(), content: result });
                        }
                        Err(e) => {
                            self.messages.push(ChatMessage::output(&format!("Tool error: {}", e)));
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
                    conversation.push(Message { role: "user".to_string(), content: output });
                }
            }
            ApprovalState::Rejected => {
                self.messages.push(ChatMessage::output("Action rejected by user"));
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
                            conversation.push(Message { role: "assistant".to_string(), content: lm_output });
                            conversation.push(Message { role: "user".to_string(), content: result });
                        }
                        Err(e) => {
                            self.messages.push(ChatMessage::output(&format!("Tool error: {}", e)));
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

// ── UI Drawing ───────────────────────────────────────────────

fn draw_ui(f: &mut Frame, app: &App) {
    let area = f.area();

    // Three-zone vertical layout: header | conversation | input area
    let zones = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // header bar
            Constraint::Min(1),    // conversation
            Constraint::Length(4), // input + hint
        ])
        .split(area);

    // ── Header bar ───────────────────────────────────────────
    draw_header(f, app, zones[0]);

    // ── Conversation area ────────────────────────────────────
    draw_conversation(f, app, zones[1]);

    // ── Input area ───────────────────────────────────────────
    draw_input(f, app, zones[2]);
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
    ]))
    .style(Style::default().bg(Color::Rgb(25, 25, 35)));

    f.render_widget(header, area);
}

fn draw_conversation(f: &mut Frame, app: &App, area: Rect) {
    let t = &*THEME;
    let width = area.width.saturating_sub(2) as usize; // account for borders

    // Render all messages into lines
    let mut all_lines: Vec<Line<'static>> = Vec::new();

    // Welcome message if no messages yet
    if app.messages.is_empty() {
        all_lines.push(Line::from(""));
        all_lines.push(Line::from(vec![
            Span::styled("  Welcome to ", t.dim),
            Span::styled("codr", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(". Type a message to get started.", t.dim),
        ]));
        all_lines.push(Line::from(""));
    }

    for msg in &app.messages {
        let rendered = render_message(msg, width);
        all_lines.extend(rendered);
    }

    // Spinner line if processing
    if app.is_processing {
        all_lines.push(Line::from(vec![
            Span::styled("  * ", Style::default().fg(Color::Cyan)),
            Span::styled("thinking...", t.dim),
        ]));
    }

    let visible_height = area.height.saturating_sub(2) as usize; // borders
    let total_lines = all_lines.len();

    // Clamp scroll offset
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = app.scroll_offset.min(max_scroll);

    // Scroll from bottom: when scroll_offset == 0, show the last lines
    let bottom_scroll = max_scroll.saturating_sub(scroll);

    let conversation = Paragraph::new(all_lines)
        .scroll((bottom_scroll as u16, 0))
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(Color::Rgb(40, 40, 55)))
        )
        .style(Style::default().bg(Color::Rgb(15, 15, 20)));

    f.render_widget(conversation, area);

    // Scroll indicator
    if scroll > 0 {
        let indicator = Paragraph::new(Line::from(vec![
            Span::styled(format!("  ^ {} more lines ", scroll), t.dim),
        ]));
        let indicator_area = Rect::new(area.x, area.y, area.width, 1);
        f.render_widget(indicator, indicator_area);
    }
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let _t = &*THEME;
    let is_pending = matches!(app.approval_state, ApprovalState::Pending);

    // Split: input line + hint line
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // input box (with borders)
            Constraint::Length(1), // hint line
        ])
        .split(area);

    // Input box
    let input_style = if app.is_processing {
        Style::default().fg(Color::DarkGray)
    } else if is_pending {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };

    let prompt_label = if is_pending {
        "approve (a) / reject (r)"
    } else if app.is_processing {
        "processing..."
    } else {
        ">"
    };

    let input = Paragraph::new(app.input.as_str())
        .style(input_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(60, 60, 80)))
                .title(Span::styled(
                    format!(" {} ", prompt_label),
                    if is_pending {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    },
                ))
        );
    f.render_widget(input, chunks[0]);

    // Cursor
    if !app.is_processing && !is_pending {
        f.set_cursor_position((
            chunks[0].x + app.input.width().min(chunks[0].width as usize - 2) as u16 + 1,
            chunks[0].y + 1,
        ));
    }

    // Hint line
    let hint = Paragraph::new(render_hint_line(is_pending))
        .style(Style::default().bg(Color::Rgb(25, 25, 35)));
    f.render_widget(hint, chunks[1]);
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
        terminal.draw(|f| draw_ui(f, app))?;

        if app.should_quit {
            break;
        }

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
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
                            if let Err(e) = app.process_message().await {
                                app.is_processing = false;
                                app.messages.push(ChatMessage::error(&format!("Error: {}", e)));
                            }
                        }
                    }
                    KeyCode::Enter => {
                        if !app.is_processing
                            && !matches!(app.approval_state, ApprovalState::Pending)
                            && !app.input.trim().is_empty()
                        {
                            if let Err(e) = app.process_message().await {
                                app.is_processing = false;
                                app.messages.push(ChatMessage::error(&format!("Error: {}", e)));
                            }
                        }
                    }

                    // -- Ctrl+C: stop agent / double = quit --
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let now = Instant::now();
                        if app.is_processing {
                            app.is_processing = false;
                            app.streaming_content.clear();
                            app.messages.push(ChatMessage::system("interrupted"));
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
