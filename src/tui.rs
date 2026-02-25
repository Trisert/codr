use crate::model::{Model, Message};
use crate::parser::{parse_action, Action, format_available_tools};
use crate::error::AgentError;
use crate::tools::ToolRegistry;
use crate::tui_components::{
    ChatMessage, MarkdownRenderer, ApprovalState, PendingAction,
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
    widgets::{Block, Borders, List, ListItem, Paragraph},
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

fn draw_chat(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(60),
        ])
        .split(area);

    draw_chat_panel(f, app, chunks[0]);
    draw_terminal_panel(f, app, chunks[1]);
    draw_status_line(f, app, area);
}

fn draw_chat_panel(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
        .split(area);

    let header_style = Style::default()
        .fg(Color::White)
        .bg(Color::Rgb(30, 30, 46))
        .add_modifier(Modifier::BOLD);

    let _header = Paragraph::new(Line::from(vec![
        Span::styled("codr", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ]))
    .block(Block::default().borders(Borders::TOP | Borders::LEFT | Borders::RIGHT).border_style(header_style));

    let messages: Vec<ListItem> = app
        .messages
        .iter()
        .rev()
        .take(50)
        .flat_map(|msg| {
            let role_style = match msg.role.as_str() {
                "user" => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                "assistant" => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                "action" => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                "output" => Style::default().fg(Color::Blue),
                "error" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                _ => Style::default(),
            };

            let role_label = match msg.role.as_str() {
                "user" => "You",
                "assistant" => "codr",
                "action" => "Action",
                "output" => "Output",
                "error" => "Error",
                _ => &msg.role,
            };

            let mut items = vec![
                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{}] ", msg.timestamp), Style::default().fg(Color::DarkGray)),
                    Span::styled(role_label, role_style),
                ])),
            ];

            for line in msg.content.lines().take(100) {
                items.push(ListItem::new(Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::from(line),
                ])));
            }

            items
        })
        .collect();

    let list = List::new(messages)
        .block(Block::default().borders(Borders::ALL).title("Chat"));
    f.render_widget(list, chunks[0]);

    let input_style = if app.is_processing {
        Style::default().fg(Color::DarkGray)
    } else if matches!(app.approval_state, ApprovalState::Pending) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };

    let input_label = match app.approval_state {
        ApprovalState::Pending => "Approve (a) / Reject (r)",
        _ => "Input (Enter to send, Ctrl+Q to quit)",
    };

    let input = Paragraph::new(app.input.as_str())
        .style(input_style)
        .block(Block::default().borders(Borders::ALL).title(input_label));
    f.render_widget(input, chunks[1]);

    f.set_cursor_position((
        chunks[1].x + app.input.width().min(chunks[1].width as usize - 2) as u16 + 1,
        chunks[1].y + 1,
    ));
}

fn draw_terminal_panel(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(1)].as_ref())
        .split(area);

    let terminal_style = Style::default().bg(Color::Rgb(10, 10, 15));

    let content: Vec<Line> = app
        .messages
        .iter()
        .rev()
        .take(100)
        .flat_map(|msg| {
            let color = match msg.role.as_str() {
                "user" => Color::Cyan,
                "assistant" => Color::Green,
                "action" => Color::Yellow,
                "output" => Color::White,
                "error" => Color::Red,
                _ => Color::Gray,
            };

            let prefix = match msg.role.as_str() {
                "user" => "> ",
                "assistant" => "",
                "action" => "$ ",
                "output" => "",
                "error" => "! ",
                _ => "",
            };

            let mut lines = Vec::new();
            for (i, line) in msg.content.lines().enumerate() {
                if i == 0 {
                    lines.push(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(color).add_modifier(Modifier::BOLD)),
                        Span::styled(line, Style::default().fg(color)),
                    ]));
                } else {
                    lines.push(Line::from(vec![Span::styled(line, Style::default().fg(color))]));
                }
            }
            lines
        })
        .collect();

    let terminal = Paragraph::new(content)
        .style(terminal_style)
        .block(Block::default().borders(Borders::ALL).title("Terminal").border_style(Style::default().fg(Color::Rgb(60, 60, 80))));
    f.render_widget(terminal, chunks[0]);
}

fn draw_status_line(f: &mut Frame, app: &App, area: Rect) {
    let status_area = Rect::new(area.x, area.height - 1, area.width, area.height);
    let status_style = Style::default().bg(Color::Rgb(40, 40, 60)).fg(Color::Gray);

    let model_name = &app.model_name;
    let tokens_str = format!("{} tokens", app.session_tokens);
    let cost_str = format!("${:.4}", app.session_cost);

    let content = format!(" {} | {} | {} ", model_name, tokens_str, cost_str);

    let status = Paragraph::new(content)
        .style(status_style)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(status, status_area);
}

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

        terminal.draw(|f| draw_chat(f, &app, f.area()))?;

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
                    KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if !app.is_processing && !matches!(app.approval_state, ApprovalState::Pending) {
                            let has_input = !app.input.trim().is_empty();
                            drop(app);
                            if has_input {
                                let mut app = app_arc.lock().unwrap();
                                app.process_message().await?;
                            }
                        }
                    }
                    KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let mut app = app_arc.lock().unwrap();
                        app.should_quit = true;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let mut app = app_arc.lock().unwrap();
                        app.should_quit = true;
                    }
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
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
