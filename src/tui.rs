use crate::model::{Model, Message};
use crate::parser::{parse_action, Action, format_available_tools};
use crate::error::AgentError;
use crate::tools::ToolRegistry;
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

// ============================================================
// Chat Message for Display
// ============================================================

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

impl ChatMessage {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: chrono_timestamp(),
        }
    }

    pub fn user(content: &str) -> Self {
        Self::new("user", content)
    }

    pub fn assistant(content: &str) -> Self {
        Self::new("assistant", content)
    }

    pub fn system(content: &str) -> Self {
        Self::new("system", content)
    }

    pub fn action(content: &str) -> Self {
        Self::new("action", content)
    }

    pub fn output(content: &str) -> Self {
        Self::new("output", content)
    }

    pub fn error(content: &str) -> Self {
        Self::new("error", content)
    }
}

fn chrono_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mins = (secs / 60) % 60;
    let hours = (secs / 3600) % 24;
    format!("{:02}:{:02}", hours, mins)
}

// ============================================================
// TUI App
// ============================================================

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
}

impl App {
    pub fn new(model: Model, tool_registry: ToolRegistry) -> Self {
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
                "user" => {
                    messages.push(Message {
                        role: "user".to_string(),
                        content: chat_msg.content.clone(),
                    });
                }
                "assistant" => {
                    messages.push(Message {
                        role: "assistant".to_string(),
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

        // Build conversation history
        let mut conversation = self.get_conversation_history();

        // Agent loop with action execution
        loop {
            // Query the LM
            let lm_output = self.model.query(&conversation).await?;
            self.messages.push(ChatMessage::assistant(&lm_output));

            // Parse the action
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

            // Display the action
            let action_display = match &action {
                Action::Bash(cmd) => format!("🔧 bash: {}", cmd),
                Action::Tool { name, params } => format!("🔧 tool: {} | {}", name, params),
            };
            self.messages.push(ChatMessage::action(&action_display));

            // Execute the action
            let output = match execute_action(&action, &self.tool_registry) {
                Ok(o) => o,
                Err(AgentError::TerminatingError(msg)) => {
                    self.messages.push(ChatMessage::error(&msg));
                    break;
                }
                Err(AgentError::TimeoutError(msg)) => msg,
                Err(AgentError::FormatError(msg)) => msg,
            };

            self.messages.push(ChatMessage::output(&output));

            // Send command output back to LM
            conversation.push(Message {
                role: "assistant".to_string(),
                content: lm_output,
            });
            conversation.push(Message {
                role: "user".to_string(),
                content: output,
            });
        }

        self.is_processing = false;
        Ok(())
    }
}

// ============================================================
// Execute Action
// ============================================================

fn execute_action(action: &Action, tool_registry: &ToolRegistry) -> Result<String, AgentError> {
    use std::process::Command;

    match action {
        Action::Bash(command) => {
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
        Action::Tool { name, params } => {
            match tool_registry.execute(name, params.clone()) {
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
                    Ok(result)
                }
                Err(e) => Ok(format!("Tool error: {}", e)),
            }
        }
    }
}

// ============================================================
// UI Rendering

// ============================================================
// UI Rendering
// ============================================================

fn draw_chat(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
        .split(area);

    // Chat history
    let messages: Vec<ListItem> = app
        .messages
        .iter()
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
                "user" => "👤 You",
                "assistant" => "🤖 codr",
                "action" => "🔧 Action",
                "output" => "📤 Output",
                "error" => "⚠️ Error",
                _ => &msg.role,
            };

            let mut items = vec![
                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{}] ", msg.timestamp), Style::default().fg(Color::DarkGray)),
                    Span::styled(role_label, role_style),
                ])),
            ];

            // Add content lines
            for line in msg.content.lines() {
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

    // Input field
    let input = Paragraph::new(app.input.as_str())
        .style(match app.is_processing {
            false => Style::default().fg(Color::White),
            true => Style::default().fg(Color::DarkGray),
        })
        .block(Block::default().borders(Borders::ALL).title("Input (Ctrl+S to send, Ctrl+Q to quit)"));
    f.render_widget(input, chunks[1]);

    // Set cursor position
    f.set_cursor_position(
        (
            chunks[1].x + app.input.width() as u16 + 2,
            chunks[1].y + 1,
        )
    );
}

// ============================================================
// Main TUI Loop
// ============================================================

pub async fn run_tui(app: App) -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Clone app for async event handling
    let app_arc = Arc::new(Mutex::new(app));
    let app_clone = app_arc.clone();

    // Spawn a task to check for processing completion
    let app_render = app_clone.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let app = app_render.lock().unwrap();
            if app.should_quit {
                break;
            }
        }
    });

    let tick_rate = Duration::from_millis(100);

    loop {
        let app = app_arc.lock().unwrap();

        // Draw UI
        terminal.draw(|f| draw_chat(f, &app, f.area()))?;

        // Check for quit
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

        drop(app); // Release lock before polling events

        // Handle events
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                let mut app = app_arc.lock().unwrap();

                if app.is_processing {
                    // Ignore input while processing
                    continue;
                }

                match key.code {
                    // Handle control keys first
                    KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Send message
                        let has_input = !app.input.trim().is_empty();
                        drop(app);
                        if has_input {
                            let mut app = app_arc.lock().unwrap();
                            app.process_message().await?;
                        }
                    }
                    KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.should_quit = true;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.should_quit = true;
                    }
                    // Regular key handling
                    KeyCode::Char(c) => {
                        let cursor = app.cursor_position;
                        app.input.insert(cursor, c);
                        app.cursor_position += 1;
                    }
                    KeyCode::Backspace => {
                        if app.cursor_position > 0 {
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
                    KeyCode::Enter => {
                        // Just insert newline, use Ctrl+S to send
                        let cursor = app.cursor_position;
                        app.input.insert(cursor, '\n');
                        app.cursor_position += 1;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

