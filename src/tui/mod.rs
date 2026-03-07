//! TUI module - New modular structure
//!
//! This module provides a refactored, modular TUI implementation
//! with the following improvements:
//! - Shared agent loop integration
//! - Unified update system (from agent module)
//! - Modern theme system
//! - Modular widget architecture

pub mod events;
pub mod markdown;
pub mod theme;
pub mod widgets;

// Re-exports
pub use events::{EventResult, handle_key_event, handle_mouse_event};
pub use theme::Theme;
pub use widgets::{BannerWidget, ConversationWidget, InputWidget, StatusWidget, ToastMessage};

// Import TuiUpdate from agent module to avoid circular dependency
use crate::agent::updates::TuiUpdate;
use crate::parser::clean_message_content;

use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::Widget,
};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::model::Message;
use crate::tools::Role;
use widgets::status::MessageLevel;

/// Check if content looks like a tool call (should be filtered from display)
fn looks_like_tool_call(content: &str) -> bool {
    let trimmed = content.trim();
    // Empty content is not a tool call
    if trimmed.is_empty() {
        return false;
    }
    // Check for JSON object starting with {"name": or {"arguments":
    if trimmed.starts_with("{\"name\":") || trimmed.starts_with("{\"arguments\":") {
        return true;
    }
    // Check for JSON array starting with [{"name":
    if trimmed.starts_with("[{\"name\":") {
        return true;
    }
    // Check for common tool call patterns
    if trimmed.contains("\"name\"")
        && (trimmed.contains("\"arguments\"") || trimmed.contains("\"parameters\""))
    {
        return true;
    }
    false
}

/// Check if content looks like a tool progress message (should be filtered from display)
/// These are internal messages like "⚙ Calling read..." or "⚙ read: generating (1.2KB)..."
fn looks_like_tool_progress(content: &str) -> bool {
    let trimmed = content.trim();
    // Check for tool progress indicator with gear emoji
    if !trimmed.starts_with("⚙") {
        return false;
    }
    // Verify it's actually a progress message, not just any message with gear emoji
    // Progress messages contain "Calling " or "generating ("
    trimmed.contains("Calling ") || trimmed.contains(": generating (")
}

/// Main TUI application state
pub struct App {
    /// Messages in conversation
    messages: Vec<Message>,

    /// Current theme
    theme: Theme,

    /// Current role (mode) - shared with background task
    role: Arc<std::sync::Mutex<Role>>,

    /// Update channel sender
    tx: mpsc::UnboundedSender<TuiUpdate>,

    /// Update channel receiver
    rx: mpsc::UnboundedReceiver<TuiUpdate>,

    /// Terminal instance
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,

    /// User input
    input: InputWidget,

    /// Toast messages
    toasts: Vec<ToastMessage>,

    /// Current progress message (optional)
    progress: Option<String>,

    /// Show spinner
    show_spinner: bool,

    /// Is agent running
    #[allow(dead_code)]
    agent_running: bool,

    /// Should exit
    should_exit: bool,

    /// Token usage
    session_tokens: u32,

    /// Session cost
    session_cost: f64,

    /// Agent status
    agent_status: widgets::banner::AgentStatus,

    /// Pending action for approval
    pending_action: Option<widgets::conversation::PendingAction>,

    /// Agent channel for user messages
    agent_tx: Option<mpsc::UnboundedSender<TuiUpdate>>,

    /// Accumulated thinking content (flushed on newlines)
    thinking_buffer: String,

    /// Last Ctrl+C press timestamp (for double-press detection)
    last_ctrl_c_press: Option<std::time::Instant>,

    /// Conversation scroll offset
    conv_scroll_offset: usize,

    /// Current conversation ID (for persistence)
    current_conversation_id: Option<String>,

    /// Model type name (for conversation metadata)
    model_type_name: String,

    /// Whether to auto-save conversations
    auto_save: bool,

    /// Collapsed output message indices
    collapsed_outputs: std::collections::HashSet<usize>,
}

impl App {
    /// Create new TUI application
    pub fn new(
        messages: Vec<Message>,
        theme: Theme,
        role: Arc<std::sync::Mutex<Role>>,
        tx: mpsc::UnboundedSender<TuiUpdate>,
        rx: mpsc::UnboundedReceiver<TuiUpdate>,
        agent_tx: Option<mpsc::UnboundedSender<TuiUpdate>>,
        model_type_name: String,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Initialize terminal
        let backend = CrosstermBackend::new(std::io::stdout());
        let terminal = Terminal::new(backend)?;

        let _cwd = std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string());

        Ok(Self {
            messages,
            theme,
            role,
            tx,
            rx,
            terminal,
            input: InputWidget::new(),
            toasts: Vec::new(),
            progress: None,
            show_spinner: false,
            agent_running: false,
            should_exit: false,
            session_tokens: 0,
            session_cost: 0.0,
            agent_status: widgets::banner::AgentStatus::Idle,
            pending_action: None,
            agent_tx,
            thinking_buffer: String::new(),
            last_ctrl_c_press: None,
            conv_scroll_offset: 0,
            current_conversation_id: None,
            model_type_name,
            auto_save: true,
            collapsed_outputs: std::collections::HashSet::new(),
        })
    }

    /// Toggle collapse state for an output message
    pub fn toggle_output_collapse(&mut self, index: usize) {
        if self.collapsed_outputs.contains(&index) {
            self.collapsed_outputs.remove(&index);
        } else {
            self.collapsed_outputs.insert(index);
        }
    }

    /// Get collapsed outputs set
    pub fn collapsed_outputs(&self) -> &std::collections::HashSet<usize> {
        &self.collapsed_outputs
    }

    /// Set the current conversation ID
    pub fn set_conversation_id(&mut self, id: String) {
        self.current_conversation_id = Some(id);
    }

    /// Get the current conversation ID
    pub fn conversation_id(&self) -> Option<&str> {
        self.current_conversation_id.as_deref()
    }

    /// Save the current conversation
    fn save_conversation(&mut self) {
        if !self.auto_save {
            return;
        }

        // Create or use existing conversation ID
        let id = self
            .current_conversation_id
            .get_or_insert_with(crate::conversation::ConversationStorage::generate_id);

        match crate::conversation::ConversationStorage::new() {
            Ok(storage) => {
                let title = self
                    .messages
                    .iter()
                    .find(|m| &*m.role == "user")
                    .and_then(|m| {
                        let content = m.content.trim();
                        if content.is_empty() {
                            None
                        } else if content.len() > 50 {
                            Some(format!("{}...", &content[..50]))
                        } else {
                            Some(content.to_string())
                        }
                    });

                match storage.save_conversation(id, &self.messages, &self.model_type_name, title) {
                    Ok(metadata) => {
                        // Only show toast on first save (new conversation)
                        if self.current_conversation_id.is_none() || self.messages.len() <= 2 {
                            self.toasts.push(ToastMessage::new(
                                format!("Conversation saved: {}", metadata.id),
                                MessageLevel::Success,
                            ));
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to save conversation: {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to initialize conversation storage: {}", e);
            }
        }
    }

    /// Create a new conversation
    fn new_conversation(&mut self) {
        // Save current conversation if it has messages
        if !self.messages.is_empty() {
            self.save_conversation();
        }

        // Clear messages and reset ID
        self.messages.clear();
        self.current_conversation_id = None;
        self.conv_scroll_offset = 0;

        self.toasts.push(ToastMessage::new(
            "Started new conversation".to_string(),
            MessageLevel::Info,
        ));
    }

    /// Load a conversation by ID
    fn load_conversation(&mut self, id: &str) {
        match crate::conversation::ConversationStorage::new() {
            Ok(storage) => match storage.load_conversation(id) {
                Ok(saved) => {
                    self.messages = saved.to_messages();
                    self.current_conversation_id = Some(saved.metadata.id.clone());
                    self.conv_scroll_offset = 0;

                    let title = saved.metadata.title.as_deref().unwrap_or("(untitled)");
                    self.toasts.push(ToastMessage::new(
                        format!("Loaded: {} - {}", id, title),
                        MessageLevel::Success,
                    ));
                }
                Err(e) => {
                    self.toasts.push(ToastMessage::new(
                        format!("Failed to load conversation: {}", e),
                        MessageLevel::Error,
                    ));
                }
            },
            Err(e) => {
                self.toasts.push(ToastMessage::new(
                    format!("Failed to access conversation storage: {}", e),
                    MessageLevel::Error,
                ));
            }
        }
    }

    /// List conversations and show as toast
    fn list_conversations(&mut self) {
        match crate::conversation::ConversationStorage::new() {
            Ok(storage) => match storage.list_conversations() {
                Ok(conversations) => {
                    if conversations.is_empty() {
                        self.toasts.push(ToastMessage::new(
                            "No saved conversations".to_string(),
                            MessageLevel::Info,
                        ));
                    } else {
                        let count = conversations.len();
                        let most_recent = conversations.first();
                        let msg = if let Some(meta) = most_recent {
                            let title = meta.title.as_deref().unwrap_or("(untitled)");
                            format!(
                                "{} conversations. Most recent: {} - {}",
                                count, meta.id, title
                            )
                        } else {
                            format!("{} conversations", count)
                        };
                        self.toasts.push(ToastMessage::new(msg, MessageLevel::Info));
                    }
                }
                Err(e) => {
                    self.toasts.push(ToastMessage::new(
                        format!("Failed to list conversations: {}", e),
                        MessageLevel::Error,
                    ));
                }
            },
            Err(e) => {
                self.toasts.push(ToastMessage::new(
                    format!("Failed to access conversation storage: {}", e),
                    MessageLevel::Error,
                ));
            }
        }
    }

    /// Delete a conversation by ID
    fn delete_conversation(&mut self, id: &str) {
        match crate::conversation::ConversationStorage::new() {
            Ok(storage) => match storage.delete_conversation(id) {
                Ok(_) => {
                    self.toasts.push(ToastMessage::new(
                        format!("Deleted conversation: {}", id),
                        MessageLevel::Success,
                    ));
                }
                Err(e) => {
                    self.toasts.push(ToastMessage::new(
                        format!("Failed to delete conversation: {}", e),
                        MessageLevel::Error,
                    ));
                }
            },
            Err(e) => {
                self.toasts.push(ToastMessage::new(
                    format!("Failed to access conversation storage: {}", e),
                    MessageLevel::Error,
                ));
            }
        }
    }

    /// Rename a conversation
    fn rename_conversation(&mut self, id: &str, new_title: String) {
        match crate::conversation::ConversationStorage::new() {
            Ok(storage) => match storage.rename_conversation(id, new_title.clone()) {
                Ok(_) => {
                    self.toasts.push(ToastMessage::new(
                        format!("Renamed conversation: {} -> \"{}\"", id, new_title),
                        MessageLevel::Success,
                    ));
                }
                Err(e) => {
                    self.toasts.push(ToastMessage::new(
                        format!("Failed to rename conversation: {}", e),
                        MessageLevel::Error,
                    ));
                }
            },
            Err(e) => {
                self.toasts.push(ToastMessage::new(
                    format!("Failed to access conversation storage: {}", e),
                    MessageLevel::Error,
                ));
            }
        }
    }

    /// Run TUI application
    pub async fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Enable raw mode
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
        crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;

        // Clear terminal
        self.terminal.clear()?;

        // Main loop
        loop {
            // Check for exit
            if self.should_exit {
                break;
            }

            // Reset Ctrl+C timer if more than 2 seconds have passed
            if let Some(last_press) = self.last_ctrl_c_press
                && last_press.elapsed().as_secs() >= 2
            {
                self.last_ctrl_c_press = None;
            }

            // Process updates
            while let Ok(update) = self.rx.try_recv() {
                self.handle_update(update);
            }

            // Process events
            if crossterm::event::poll(std::time::Duration::from_millis(10))? {
                match crossterm::event::read()? {
                    crossterm::event::Event::Key(key) => {
                        self.handle_key(key);
                    }
                    crossterm::event::Event::Mouse(mouse) => {
                        let result = handle_mouse_event(mouse);
                        match result {
                            EventResult::ScrollUp => {
                                self.conv_scroll_offset = self.conv_scroll_offset.saturating_add(1);
                            }
                            EventResult::ScrollDown => {
                                self.conv_scroll_offset = self.conv_scroll_offset.saturating_sub(1);
                            }
                            EventResult::ScrollToTop => {
                                let max_offset = self.messages.len().saturating_sub(20);
                                self.conv_scroll_offset = max_offset;
                            }
                            EventResult::ScrollToBottom => {
                                self.conv_scroll_offset = 0;
                            }
                            _ => {}
                        }
                    }
                    crossterm::event::Event::Resize(_, _) => {
                        // Terminal resized - re-render will happen automatically
                        // on next loop iteration with updated terminal size
                    }
                    _ => {
                        // Ignore other events
                    }
                }
            }

            // Render
            self.terminal.draw(|f| {
                let size = f.area();
                Self::render_static(RenderParams {
                    size,
                    buf: f.buffer_mut(),
                    theme: &self.theme,
                    role_name: self.role.lock().unwrap().name(),
                    messages: &self.messages,
                    scroll_offset: self.conv_scroll_offset,
                    input_text: self.input.input(),
                    toasts: &self.toasts,
                    progress: self.progress.as_deref(),
                    show_spinner: self.show_spinner,
                    pending_action: self.pending_action.as_ref(),
                    agent_status: self.agent_status,
                    session_tokens: self.session_tokens,
                    session_cost: self.session_cost,
                    collapsed_outputs: &self.collapsed_outputs,
                });
            })?;

            // Dismiss expired toasts
            self.dismiss_expired_toasts();

            // Small delay to prevent busy loop
            tokio::time::sleep(std::time::Duration::from_millis(16)).await;
        }

        // Cleanup
        self.cleanup()?;

        Ok(())
    }

    /// Handle update from agent loop
    fn handle_update(&mut self, update: TuiUpdate) {
        match update {
            TuiUpdate::ActionMessage(msg) => {
                // Show action message (filtered to remove XML tags)
                let cleaned = clean_message_content(&msg, true);
                self.messages.push(Message {
                    role: "action".into(),
                    content: Arc::new(cleaned),
                    images: Vec::new(),
                    metadata: None,
                });
                // Auto-scroll to bottom
                self.conv_scroll_offset = 0;
                // Auto-save after message
                self.save_conversation();
            }

            TuiUpdate::ToolProgress(progress) => {
                // Don't show progress messages in status bar - they clutter the UI
                let _ = progress; // Suppress unused warning
                // Still update agent status
                self.agent_status = widgets::banner::AgentStatus::Running;
            }

            TuiUpdate::OutputMessage {
                content: output,
                tool_category,
            } => {
                // Skip empty output messages (e.g., from read tool)
                if output.trim().is_empty() {
                    return;
                }
                // Show tool output with category metadata
                self.messages.push(Message {
                    role: "output".into(),
                    content: output,
                    images: Vec::new(),
                    metadata: tool_category.map(|cat| crate::model::MessageMetadata {
                        tool_category: Some(cat),
                    }),
                });
                // Auto-scroll to bottom
                self.conv_scroll_offset = 0;
                // Auto-save after message
                self.save_conversation();
            }

            TuiUpdate::ErrorMessage(error) => {
                // Show error toast and set status
                self.toasts
                    .push(ToastMessage::new(error.to_string(), MessageLevel::Error));
                self.agent_status = widgets::banner::AgentStatus::Error;
            }

            TuiUpdate::NeedsApproval {
                action_type,
                content,
                is_tool,
            } => {
                // Set pending action for approve/reject workflow
                let _ = is_tool; // Not currently used
                self.pending_action = Some(widgets::conversation::PendingAction {
                    action_type: action_type.to_string(),
                    content: (*content).clone(),
                });
            }

            TuiUpdate::StreamingContent { role, content } => {
                // Update or create streaming message
                self.agent_status = widgets::banner::AgentStatus::Streaming;

                // Clean content: filter XML tool tags and JSON tool calls
                let cleaned = clean_message_content(&content, false);

                // Filter out tool call JSON and progress messages from display content
                // Tool calls look like: {"name": "...", "arguments": {...}} or arrays of such objects
                // Progress messages look like: "⚙ Calling read..." or "⚙ read: generating (1.2KB)..."
                let filtered_content = if cleaned.trim().is_empty()
                    || looks_like_tool_call(&cleaned)
                    || looks_like_tool_progress(&cleaned)
                {
                    String::new() // Don't display tool calls or progress messages as content
                } else {
                    cleaned
                };

                let is_new_message = if let Some(last_msg) = self.messages.last_mut() {
                    // Check if we should append to existing message or create new one
                    if *last_msg.role == *role {
                        // Same role
                        if filtered_content.is_empty() {
                            // Empty content: nothing to add
                            false
                        } else {
                            // Non-empty content: append to existing message
                            let mut existing = (*last_msg.content).clone();
                            existing.push_str(&filtered_content);
                            last_msg.content = Arc::new(existing);
                            false
                        }
                    } else {
                        // Different role
                        if filtered_content.is_empty() {
                            // Empty content: don't create message yet
                            false
                        } else {
                            // Non-empty content: create new message
                            self.messages.push(Message {
                                role: role.to_string().into(),
                                content: Arc::new(filtered_content),
                                images: Vec::new(),
                                metadata: None,
                            });
                            true
                        }
                    }
                } else {
                    // First message: create it only if there's actual content
                    if !filtered_content.is_empty() {
                        self.messages.push(Message {
                            role: role.to_string().into(),
                            content: Arc::new(filtered_content),
                            images: Vec::new(),
                            metadata: None,
                        });
                        true
                    } else {
                        false
                    }
                };

                // Auto-scroll to bottom when new message is added or role changes
                if is_new_message {
                    self.conv_scroll_offset = 0;
                }
            }

            TuiUpdate::ThinkingContent(content) => {
                // Accumulate thinking content
                self.thinking_buffer.push_str(&content);

                // Flush on newline to create natural sentence chunks
                if self.thinking_buffer.contains('\n') {
                    let parts: Vec<&str> = self.thinking_buffer.splitn(2, '\n').collect();
                    if let Some(first_line) = parts.first()
                        && !first_line.is_empty()
                    {
                        self.messages.push(Message {
                            role: "thinking".into(),
                            content: Arc::new(first_line.to_string()),
                            images: Vec::new(),
                            metadata: None,
                        });
                        // Auto-scroll to bottom
                        self.conv_scroll_offset = 0;
                    }
                    // Keep the remainder (after the newline) in the buffer
                    self.thinking_buffer = parts.get(1).unwrap_or(&"").to_string();
                }
            }

            TuiUpdate::StreamingComplete { role } => {
                // Streaming done - flush any remaining thinking buffer
                if !self.thinking_buffer.is_empty() {
                    self.messages.push(Message {
                        role: "thinking".into(),
                        content: Arc::new(std::mem::take(&mut self.thinking_buffer)),
                        images: Vec::new(),
                        metadata: None,
                    });
                    // Auto-scroll to bottom
                    self.conv_scroll_offset = 0;
                }
                self.agent_status = widgets::banner::AgentStatus::Idle;
                let _ = role;
                // Auto-save after streaming completes
                self.save_conversation();
            }

            TuiUpdate::UserMessage { content } => {
                // User submitted a message (forward to agent)
                // This is handled by the agent loop, not here
                let _ = content;
            }

            TuiUpdate::ActionApproved {
                action_type,
                content,
            } => {
                // Action approved (forward to agent)
                // This is handled by the agent loop, not here
                let _ = (action_type, content);
            }

            TuiUpdate::ActionRejected => {
                // Action rejected (forward to agent)
                // This is handled by the agent loop, not here
            }

            TuiUpdate::InterruptAgent => {
                // Agent interrupted - already handled in background loop
                // Ignore here
            }

            TuiUpdate::UsageUpdate {
                input_tokens,
                output_tokens,
                cost,
            } => {
                // Update token usage and cost
                self.session_tokens = input_tokens + output_tokens;
                self.session_cost = cost;
            }
        }
    }

    /// Handle key event
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        match handle_key_event(key) {
            EventResult::InterruptAgent => {
                // Handle Ctrl+C: first press interrupts, second press exits
                let now = std::time::Instant::now();
                let should_exit = if let Some(last_press) = self.last_ctrl_c_press {
                    // Second Ctrl+C within 2 seconds -> exit
                    now.duration_since(last_press).as_secs() < 2
                } else {
                    false
                };

                if should_exit {
                    // Second Ctrl+C - exit application
                    self.should_exit = true;
                } else {
                    // First Ctrl+C - interrupt agent
                    self.last_ctrl_c_press = Some(now);
                    self.interrupt_agent();
                    self.toasts.push(ToastMessage::new(
                        "Agent interrupted. Press Ctrl+C again within 2s to exit.".to_string(),
                        MessageLevel::Warning,
                    ));
                }
            }

            EventResult::Cancel => {
                // Cancel current action
                self.input.clear();
                self.progress = None;
                self.show_spinner = false;
            }

            EventResult::Exit => {
                // Exit application (Ctrl+D)
                self.should_exit = true;
            }

            EventResult::SwitchRole => {
                // Switch role (shift-tab or tab)
                let mut role = self.role.lock().unwrap();
                *role = match *role {
                    Role::Safe => Role::Yolo,
                    Role::Yolo => Role::Safe,
                    Role::Planning => Role::Safe,
                };
                drop(role); // Release lock before formatting

                // Remove any existing role switch toasts
                self.toasts
                    .retain(|t| !t.text.contains("Switched to") && !t.text.contains("mode"));

                // Show toast
                let role_name = self.role.lock().unwrap().name();
                self.toasts.push(ToastMessage::new(
                    format!("Switched to {} mode", role_name),
                    MessageLevel::Info,
                ));
            }

            EventResult::Input(ch) => {
                // Check if this is an approve/reject key when there's a pending action
                let has_pending_action = self.pending_action.is_some();

                if has_pending_action && ch == 'a' {
                    // Approve pending action
                    if let Some(action) = self.pending_action.take() {
                        if let Err(e) = self.tx.send(TuiUpdate::ActionApproved {
                            action_type: action.action_type,
                            content: Arc::new(action.content),
                        }) {
                            self.toasts.push(ToastMessage::new(
                                format!("Failed to approve action: {}", e),
                                MessageLevel::Error,
                            ));
                        }
                        self.toasts.push(ToastMessage::new(
                            "Action approved".to_string(),
                            MessageLevel::Success,
                        ));
                    }
                } else if has_pending_action && ch == 'r' {
                    // Reject pending action
                    if self.pending_action.take().is_some() {
                        if let Err(e) = self.tx.send(TuiUpdate::ActionRejected) {
                            self.toasts.push(ToastMessage::new(
                                format!("Failed to reject action: {}", e),
                                MessageLevel::Error,
                            ));
                        }
                        self.toasts.push(ToastMessage::new(
                            "Action rejected".to_string(),
                            MessageLevel::Info,
                        ));
                    }
                } else {
                    // Type character normally
                    self.input.insert(ch);
                }
            }

            EventResult::Backspace => {
                // Delete character before cursor
                self.input.backspace();
            }

            EventResult::Delete => {
                // Delete character after cursor
                self.input.delete();
            }

            EventResult::MoveLeft => {
                // Move cursor left
                self.input.move_left();
            }

            EventResult::MoveRight => {
                // Move cursor right
                self.input.move_right();
            }

            EventResult::MoveToStart => {
                // Move cursor to start
                self.input.move_to_start();
            }

            EventResult::MoveToEnd => {
                // Move cursor to end
                self.input.move_to_end();
            }

            EventResult::HistoryUp => {
                // Navigate history up
                self.input.history_up();
            }

            EventResult::HistoryDown => {
                // Navigate history down
                self.input.history_down();
            }

            EventResult::ScrollUp => {
                // Scroll up (hide newest messages, see older ones)
                self.conv_scroll_offset = self.conv_scroll_offset.saturating_add(1);
            }

            EventResult::ScrollDown => {
                // Scroll down (show more newest messages)
                self.conv_scroll_offset = self.conv_scroll_offset.saturating_sub(1);
            }

            EventResult::ScrollToTop => {
                // Scroll to top (hide all except oldest ~20 messages)
                let max_offset = self.messages.len().saturating_sub(20);
                self.conv_scroll_offset = max_offset;
            }

            EventResult::ScrollToBottom => {
                // Scroll to bottom (show all messages, newest at bottom)
                self.conv_scroll_offset = 0;
            }

            EventResult::Submit => {
                // Submit input
                if let Some(text) = self.input.submit() {
                    let text = text.trim();

                    // Check for conversation management commands
                    if let Some(cmd) = text.strip_prefix('/') {
                        let parts: Vec<&str> = cmd.split_whitespace().collect();
                        let command = parts.first().copied().unwrap_or("");

                        match command {
                            "new" | "n" => {
                                self.new_conversation();
                            }
                            "list" | "ls" | "conv" | "conversations" => {
                                self.list_conversations();
                            }
                            "load" | "l" => {
                                if parts.len() >= 2 {
                                    let id = parts[1];
                                    self.load_conversation(id);
                                } else {
                                    self.toasts.push(ToastMessage::new(
                                        "Usage: /load <conversation_id>".to_string(),
                                        MessageLevel::Info,
                                    ));
                                }
                            }
                            "delete" | "del" | "d" => {
                                if parts.len() >= 2 {
                                    let id = parts[1];
                                    self.delete_conversation(id);
                                } else {
                                    self.toasts.push(ToastMessage::new(
                                        "Usage: /delete <conversation_id>".to_string(),
                                        MessageLevel::Info,
                                    ));
                                }
                            }
                            "rename" | "r" => {
                                if parts.len() >= 3 {
                                    let id = parts[1];
                                    let new_title = parts[2..].join(" ");
                                    self.rename_conversation(id, new_title);
                                } else {
                                    self.toasts.push(ToastMessage::new(
                                        "Usage: /rename <conversation_id> <new_title>".to_string(),
                                        MessageLevel::Info,
                                    ));
                                }
                            }
                            "export" | "e" => {
                                if parts.len() >= 2 {
                                    let id = parts[1];
                                    let format = if parts.len() >= 3 {
                                        parts[2].parse::<crate::conversation::ExportFormat>().ok()
                                    } else {
                                        None
                                    };

                                    let format = format
                                        .unwrap_or(crate::conversation::ExportFormat::Markdown);
                                    let output_path = std::path::PathBuf::from(format!(
                                        "{}.{}",
                                        id,
                                        format.extension()
                                    ));

                                    match crate::conversation::ConversationStorage::new() {
                                        Ok(storage) => {
                                            match storage.export_conversation(
                                                id,
                                                format,
                                                &output_path,
                                            ) {
                                                Ok(_) => {
                                                    self.toasts.push(ToastMessage::new(
                                                        format!(
                                                            "Exported: {} ({:?})",
                                                            output_path.display(),
                                                            format
                                                        ),
                                                        MessageLevel::Success,
                                                    ));
                                                }
                                                Err(e) => {
                                                    self.toasts.push(ToastMessage::new(
                                                        format!("Failed to export: {}", e),
                                                        MessageLevel::Error,
                                                    ));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            self.toasts.push(ToastMessage::new(
                                                format!("Failed to access storage: {}", e),
                                                MessageLevel::Error,
                                            ));
                                        }
                                    }
                                } else {
                                    self.toasts.push(ToastMessage::new(
                                        "Usage: /export <conversation_id> [format]".to_string(),
                                        MessageLevel::Info,
                                    ));
                                }
                            }
                            "help" | "h" | "?" => {
                                self.toasts.push(ToastMessage::new(
                                    "Commands: /new, /list, /load <id>, /delete <id>, /rename <id> <title>, /export <id> [format]".to_string(),
                                    MessageLevel::Info,
                                ));
                            }
                            _ => {
                                self.toasts.push(ToastMessage::new(
                                    format!(
                                        "Unknown command: /{}. Use /help for available commands",
                                        command
                                    ),
                                    MessageLevel::Warning,
                                ));
                            }
                        }
                    } else if !text.is_empty() {
                        // Add user message to conversation
                        self.messages.push(Message {
                            role: "user".into(),
                            content: Arc::new(text.to_string()),
                            images: Vec::new(),
                            metadata: None,
                        });

                        // Auto-save after user message
                        self.save_conversation();

                        // Send to agent for processing (only once)
                        if let Some(ref agent_tx) = self.agent_tx
                            && let Err(e) = agent_tx.send(TuiUpdate::UserMessage {
                                content: Arc::new(text.to_string()),
                            })
                        {
                            self.toasts.push(ToastMessage::new(
                                format!("Failed to send message: {}", e),
                                MessageLevel::Error,
                            ));
                        }

                        // Show a toast to confirm submission
                        self.toasts.push(ToastMessage::new(
                            "Message sent".to_string(),
                            MessageLevel::Success,
                        ));
                    }
                }
            }

            EventResult::ApproveAction => {
                // Not used anymore - handled in Input case
            }

            EventResult::RejectAction => {
                // Not used anymore - handled in Input case
            }

            EventResult::ToggleOutputCollapse => {
                // Toggle collapse for the last output message
                if let Some(last_output_idx) =
                    self.messages.iter().rposition(|m| &*m.role == "output")
                {
                    self.toggle_output_collapse(last_output_idx);
                    let is_collapsed = self.collapsed_outputs().contains(&last_output_idx);
                    self.toasts.push(ToastMessage::new(
                        if is_collapsed {
                            "Output collapsed".to_string()
                        } else {
                            "Output expanded".to_string()
                        },
                        MessageLevel::Info,
                    ));
                }
            }

            EventResult::NoOp => {}
        }
    }

    /// Render TUI (not used directly, but kept for API compatibility)
    #[allow(dead_code)]
    fn render(&self, size: Rect, buf: &mut ratatui::buffer::Buffer) {
        Self::render_static(RenderParams {
            size,
            buf,
            theme: &self.theme,
            role_name: self.role.lock().unwrap().name(),
            messages: &self.messages,
            scroll_offset: self.conv_scroll_offset,
            input_text: self.input.input(),
            toasts: &self.toasts,
            progress: self.progress.as_deref(),
            show_spinner: self.show_spinner,
            pending_action: self.pending_action.as_ref(),
            agent_status: self.agent_status,
            session_tokens: self.session_tokens,
            session_cost: self.session_cost,
            collapsed_outputs: &self.collapsed_outputs,
        });
    }

    /// Static render function
    fn render_static(params: RenderParams<'_>) {
        let RenderParams {
            size,
            buf,
            theme,
            role_name,
            messages,
            scroll_offset,
            input_text,
            toasts,
            progress,
            show_spinner,
            pending_action,
            agent_status,
            session_tokens,
            session_cost,
            collapsed_outputs,
        } = params;

        // Create layout - Codex style: banner (2 lines), conversation, input (2 lines)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Banner (2 lines + separator)
                Constraint::Min(0),    // Conversation
                Constraint::Length(3), // Input
            ])
            .split(size);

        // Render banner with enhanced status
        let banner = BannerWidget::new(theme, "codr", role_name)
            .tokens(session_tokens)
            .cost(session_cost)
            .cwd(None)
            .agent_status(agent_status)
            .connected(true);

        banner.render(chunks[0], buf);

        // Render conversation
        let conv_widget = ConversationWidget::new(messages, theme)
            .scroll_offset(scroll_offset)
            .pending_action(pending_action.cloned())
            .collapsed_outputs(collapsed_outputs.clone());
        conv_widget.render(chunks[1], buf);

        // Render input
        let mut input_widget = InputWidget::new().with_theme(*theme).focused(true);
        input_widget.set_input(input_text);
        input_widget.render(chunks[2], buf);

        // Render status toasts (overlay)
        if !toasts.is_empty() || progress.is_some() || show_spinner {
            let status_widget = StatusWidget::new(theme, toasts)
                .progress(progress)
                .spinner(show_spinner);
            status_widget.render(size, buf);
        }
    }

    /// Dismiss expired toasts
    fn dismiss_expired_toasts(&mut self) {
        self.toasts.retain(|t| !t.should_dismiss());
    }

    /// Interrupt the current agent operation
    fn interrupt_agent(&mut self) {
        // Clear any pending action
        self.pending_action = None;

        // Reset agent status
        self.agent_status = widgets::banner::AgentStatus::Idle;

        // Clear progress and spinner
        self.progress = None;
        self.show_spinner = false;

        // Flush any remaining thinking buffer
        if !self.thinking_buffer.is_empty() {
            self.messages.push(Message {
                role: "thinking".into(),
                content: Arc::new(std::mem::take(&mut self.thinking_buffer)),
                images: Vec::new(),
                metadata: None,
            });
        }

        // Add a message to show the agent was interrupted
        // Send interruption signal to background agent task
        let _ = self.tx.send(TuiUpdate::InterruptAgent);
        self.messages.push(Message {
            role: "action".into(),
            content: Arc::new("Agent operation interrupted".to_string()),
            images: Vec::new(),
            metadata: None,
        });
    }

    /// Cleanup terminal
    fn cleanup(&mut self) -> std::io::Result<()> {
        // Save conversation before exiting
        if !self.messages.is_empty() {
            self.save_conversation();
        }

        crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
        crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture)?;
        crossterm::terminal::disable_raw_mode()?;
        Ok(())
    }

    /// Set theme
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Get theme
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Get messages (for export commands)
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }
}

/// Parameters for the static render function
struct RenderParams<'a> {
    size: Rect,
    buf: &'a mut ratatui::buffer::Buffer,
    theme: &'a Theme,
    role_name: &'static str,
    messages: &'a [Message],
    scroll_offset: usize,
    input_text: &'a str,
    toasts: &'a [ToastMessage],
    progress: Option<&'a str>,
    show_spinner: bool,
    pending_action: Option<&'a widgets::conversation::PendingAction>,
    agent_status: widgets::banner::AgentStatus,
    session_tokens: u32,
    session_cost: f64,
    collapsed_outputs: &'a std::collections::HashSet<usize>,
}

/// Run TUI with integrated agent loop
pub async fn run_tui_agent(
    model: crate::model::Model,
    tool_registry: std::sync::Arc<crate::tools::ToolRegistry>,
    initial_messages: Vec<Message>,
    role: Role,
    resume: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get model type name for conversation metadata
    let model_type_name = match model.model_type() {
        crate::model::ModelType::Anthropic => "anthropic".to_string(),
        crate::model::ModelType::OpenAI { model, .. } => model.clone(),
    };

    // Load most recent conversation if resume flag is set and no initial messages provided
    let (initial_messages, initial_conversation_id) = if resume && initial_messages.is_empty() {
        match crate::conversation::ConversationStorage::new() {
            Ok(storage) => match storage.get_most_recent() {
                Ok(Some(saved)) => {
                    let messages = saved.to_messages();
                    let conversation_id = saved.metadata.id.clone();
                    eprintln!("Resumed conversation: {}", conversation_id);
                    (messages, Some(conversation_id))
                }
                Ok(None) => {
                    eprintln!("No saved conversation found, starting fresh");
                    (Vec::new(), None)
                }
                Err(e) => {
                    eprintln!("Warning: Failed to load recent conversation: {}", e);
                    (Vec::new(), None)
                }
            },
            Err(e) => {
                eprintln!("Warning: Failed to access conversation storage: {}", e);
                (Vec::new(), None)
            }
        }
    } else {
        (initial_messages, None)
    };

    // Create update channels
    let (tx, rx) = mpsc::unbounded_channel();

    // Create separate channel for agent task
    let (agent_tx, mut agent_rx) = mpsc::unbounded_channel();

    // Wrap role in Arc<Mutex> for sharing between app and background task
    let role = Arc::new(std::sync::Mutex::new(role));

    // Create TUI app (tx is moved here, so we need to clone before)
    let tx_for_callbacks = tx.clone();
    let tx_for_app = tx.clone();
    let theme = Theme::dark();
    let initial_messages_for_app = initial_messages.clone();
    let mut app = App::new(
        initial_messages_for_app,
        theme,
        role.clone(),
        tx_for_app,
        rx,
        Some(agent_tx),
        model_type_name,
    )?;

    // Set the conversation ID if we loaded one
    if let Some(id) = initial_conversation_id {
        app.set_conversation_id(id);
    }

    // Define streaming callbacks
    let tx_streaming = tx_for_callbacks.clone();
    let _on_streaming: crate::agent::StreamingCallback = Arc::new(move |content| {
        let _ = tx_streaming.send(TuiUpdate::StreamingContent {
            role: "assistant".into(),
            content,
        });
    });

    let tx_thinking = tx_for_callbacks.clone();
    let _on_thinking: crate::agent::ThinkingCallback = Arc::new(move |content| {
        let _ = tx_thinking.send(TuiUpdate::ThinkingContent(content));
    });

    // Run agent loop in background task
    let tx_ui = tx.clone();
    use crate::agent::{LoopConfig, TUIExecutor, run_agent_loop};

    tokio::spawn(async move {
        // Track conversation history
        let mut conversation = initial_messages;

        // Define streaming callbacks
        let tx_streaming = tx.clone();
        let on_streaming: crate::agent::StreamingCallback = Arc::new(move |content| {
            let _ = tx_streaming.send(TuiUpdate::StreamingContent {
                role: "assistant".into(),
                content,
            });
        });

        let tx_thinking = tx.clone();
        let on_thinking: crate::agent::ThinkingCallback = Arc::new(move |content| {
            let _ = tx_thinking.send(TuiUpdate::ThinkingContent(content));
        });

        // Main agent loop
        loop {
            // Wait for user message
            let user_message = match agent_rx.recv().await {
                Some(TuiUpdate::UserMessage { content }) => content,
                Some(TuiUpdate::ActionApproved {
                    action_type,
                    content,
                }) => {
                    // Handle action approval
                    let _ = tx_ui.send(TuiUpdate::OutputMessage {
                        content: Arc::new(format!("✓ Approved: {} - {}", action_type, content)),
                        tool_category: None,
                    });

                    // Continue agent processing after approval
                    // For now, just acknowledge - real implementation would resume agent loop
                    continue;
                }
                Some(TuiUpdate::ActionRejected) => {
                    // Handle action rejection
                    let _ = tx_ui.send(TuiUpdate::OutputMessage {
                        content: Arc::new("✗ Action rejected".to_string()),
                        tool_category: None,
                    });
                    // Stop current operation
                    continue;
                }
                None => {
                    // Channel closed
                    break;
                }
                Some(TuiUpdate::InterruptAgent) => {
                    // Agent interrupted - break out of loop
                    break;
                }
                _ => {
                    // Ignore other updates
                    continue;
                }
            };

            // Add user message to conversation
            conversation.push(Message {
                role: "user".into(),
                content: user_message.clone(),
                images: Vec::new(),
                metadata: None,
            });

            // Run agent loop with streaming support
            // Create a new executor for this iteration
            let current_role = *role.lock().unwrap();
            let executor = TUIExecutor::new(tool_registry.clone(), tx.clone(), current_role);
            let config = LoopConfig::streaming(on_streaming.clone(), on_thinking.clone());
            match run_agent_loop(
                &model,
                conversation.clone(),
                &tool_registry,
                executor,
                &current_role,
                config,
            )
            .await
            {
                Ok(result) => {
                    // Update conversation with new messages from agent
                    conversation = result.conversation;

                    // Show completion message
                    if result.final_response.is_some() {
                        let _ = tx_ui.send(TuiUpdate::StreamingComplete {
                            role: "assistant".into(),
                        });
                    }
                }
                Err(error) => {
                    // Show error to user
                    let _ = tx_ui.send(TuiUpdate::ErrorMessage(
                        format!("Agent error: {}", error).into(),
                    ));
                    let _ = tx_ui.send(TuiUpdate::StreamingComplete {
                        role: "assistant".into(),
                    });
                }
            }
        }
    });

    // Run TUI display
    app.run().await?;

    Ok(())
}
