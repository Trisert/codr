// ── Slash Command System ─────────────────────────────────────────────

use crate::tui::App;
use std::sync::Arc;

/// Trait for slash commands that can be executed in the TUI.
pub trait Command: Send + Sync {
    /// Return the command name (without the leading slash)
    fn name(&self) -> &str;

    /// Return a short description of what the command does
    fn description(&self) -> &str;

    /// Execute the command with access to the App state
    /// Returns Ok(message) on success, Err(message) on failure
    fn execute(&self, app: &mut App) -> Result<String, String>;
}

/// Registry for slash commands
pub struct CommandRegistry {
    commands: Vec<Arc<dyn Command>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Register a new command
    pub fn register(&mut self, command: Arc<dyn Command>) {
        self.commands.push(command);
    }

    /// Get all registered commands as (name, description) tuples
    pub fn all_commands(&self) -> Vec<(String, String)> {
        self.commands
            .iter()
            .map(|cmd| (cmd.name().to_string(), cmd.description().to_string()))
            .collect()
    }

    /// Get all command names (for fuzzy matching)
    pub fn command_names(&self) -> Vec<String> {
        self.commands
            .iter()
            .map(|cmd| cmd.name().to_string())
            .collect()
    }

    /// Execute a command by name
    pub fn execute(&self, name: &str, app: &mut App) -> Result<String, String> {
        for cmd in &self.commands {
            if cmd.name() == name {
                return cmd.execute(app);
            }
        }
        Err(format!("Unknown command: {}", name))
    }

    /// Get command by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Command>> {
        for cmd in &self.commands {
            if cmd.name() == name {
                return Some(Arc::clone(cmd));
            }
        }
        None
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Built-in Commands ────────────────────────────────────────────────

/// /copychat - Copy all messages in markdown format to clipboard
pub struct CopyChatCommand;

impl Command for CopyChatCommand {
    fn name(&self) -> &str {
        "copychat"
    }

    fn description(&self) -> &str {
        "Copy all messages in markdown format to clipboard"
    }

    fn execute(&self, app: &mut App) -> Result<String, String> {
        let mut markdown = String::new();
        let mut message_num = 0;

        for msg in app.messages() {
            match &*msg.role {
                "user" => {
                    message_num += 1;
                    markdown.push_str(&format!("## User Message {}\n", message_num));
                    markdown.push_str(&msg.content);
                    markdown.push_str("\n\n");
                }
                "assistant" => {
                    // TODO: Add thinking support (message struct doesn't have thinking field yet)
                    // if let Some(ref thinking) = msg.thinking {
                    //     markdown.push_str("**Thinking:** ");
                    //     markdown.push_str(thinking);
                    //     markdown.push_str("\n\n");
                    // }
                    markdown.push_str(&msg.content);
                    markdown.push_str("\n\n");
                }
                "action" => {
                    markdown.push_str("## Action\n");
                    // Format action content - detect if it's a bash or tool action
                    let content = &msg.content;
                    if content.starts_with("bash:") {
                        markdown.push_str("```bash\n");
                        markdown.push_str(content.strip_prefix("bash: ").unwrap_or(content));
                        markdown.push_str("\n```\n\n");
                    } else if content.starts_with("read ") || content.starts_with("edit ") || content.starts_with("write ") {
                        markdown.push_str("```\n");
                        markdown.push_str(content);
                        markdown.push_str("\n```\n\n");
                    } else {
                        markdown.push_str(content);
                        markdown.push_str("\n\n");
                    }
                }
                "output" => {
                    markdown.push_str("## Output\n");
                    // Detect if output looks like code or command output
                    let content = &msg.content;
                    if content.lines().count() > 1 || content.len() > 100 {
                        markdown.push_str("```\n");
                        markdown.push_str(content);
                        markdown.push_str("\n```\n\n");
                    } else {
                        markdown.push_str(content);
                        markdown.push_str("\n\n");
                    }
                }
                "error" => {
                    markdown.push_str("**Error:** ");
                    markdown.push_str(&msg.content);
                    markdown.push_str("\n\n");
                }
                "info" => {
                    markdown.push_str("*Info: ");
                    markdown.push_str(&msg.content);
                    markdown.push_str("*\n\n");
                }
                "system" => {
                    // Skip system messages in export
                    continue;
                }
                "diff" => {
                    markdown.push_str("## Diff\n");
                    markdown.push_str("```diff\n");
                    markdown.push_str(&msg.content);
                    markdown.push_str("\n```\n\n");
                }
                _ => {
                    // Unknown message type
                    markdown.push_str(&msg.content);
                    markdown.push_str("\n\n");
                }
            }
        }

        // Use the app's copy_to_clipboard function
        // We need to make this work - let's use the same implementation
        let success = copy_to_clipboard(&markdown);

        if success {
            let char_count = markdown.chars().count();
            let line_count = markdown.lines().count();
            Ok(format!("Copied {} chars ({} lines) to clipboard", char_count, line_count))
        } else {
            Err("Copy failed - install xclip (Linux) or use mouse selection".to_string())
        }
    }
}

/// Helper function to copy text to clipboard (duplicated from tui.rs)
fn copy_to_clipboard(text: &str) -> bool {
    use clipboard::ClipboardProvider;
    use std::io::{self, Write};
    use std::process::Command;

    // Check if we're on Wayland
    let is_wayland = std::env::var("XDG_SESSION_TYPE")
        .map(|s| s == "wayland")
        .unwrap_or(false);

    // Method 1: On Wayland, prefer wl-copy directly (more reliable)
    #[cfg(target_os = "linux")]
    if is_wayland
        && Command::new("wl-copy")
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

    // Method 2: Try clipboard crate
    if let Ok(mut ctx) = clipboard::ClipboardContext::new()
        && ctx.set_contents(text.to_string()).is_ok()
    {
        return true;
    }

    // Method 3: Try OSC 52 escape sequence (works in kitty, wezterm, iterm2)
    if let Ok(encoded) = simple_base64_encode(text) {
        let osc52 = format!("\x1b]52;c;{}\x07", encoded);
        if io::stdout().write_all(osc52.as_bytes()).is_ok() {
            let _ = io::stdout().flush();
            return true;
        }
    }

    // Method 4: Direct xclip command (most reliable on Linux X11)
    #[cfg(target_os = "linux")]
    {
        if Command::new("xclip")
            .arg("-selection")
            .arg("clipboard")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .and_then(|mut child| {
                let mut stdin = child.stdin.as_ref().unwrap();
                stdin.write_all(text.as_bytes())?;
                stdin.flush()?;
                let _ = stdin;
                child.wait()
            })
            .is_ok()
        {
            return true;
        }
    }

    false
}

/// Simple base64 encoding for OSC 52 clipboard
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

/// Create a new command registry with all built-in commands registered
pub fn create_command_registry() -> CommandRegistry {
    let mut registry = CommandRegistry::new();
    registry.register(Arc::new(CopyChatCommand));
    registry
}
