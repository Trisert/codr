# codr Improvement Specification
## Based on pi Agent Architecture Research

---

This document outlines improvements for codr based on Mario Zechner's "pi" coding agent research (https://mariozechner.at/posts/2025-11-30-pi-coding-agent/).

---

## 1. Flicker-Free TUI Rendering

### Problem
Current rendering uses ratatui's default immediate-mode approach which redraws the entire frame each tick (100ms), causing visible flicker during streaming.

### Solution
Implement synchronized output and differential rendering as described in pi-tui.

### Implementation Details

#### 1.1 Synchronized Output
Add escape sequences to enable terminal output buffering:

**File: `src/tui.rs`**

```rust
// Add to imports
use crossterm::{execute, terminal::SetTitle};

// Enable synchronized output during initialization
fn enable_synchronized_output(stdout: &mut io::Stdout) -> io::Result<()> {
    execute!(stdout, r#"\x1b[?2026h"#)
}

fn disable_synchronized_output(stdout: &mut io::Stdout) -> io::Result<()> {
    execute!(stdout, r#"\x1b[?2026l"#)
}
```

Wrap the render loop:
```rust
// In run_event_loop around line 2474
loop {
    app.poll_updates();
    app.animation_frame = app.animation_frame.wrapping_add(1);
    
    enable_synchronized_output(&mut io::stdout())?;
    terminal.draw(|f| draw_ui(f, app))?;
    disable_synchronized_output(&mut io::stdout())?;
    
    if app.should_quit { break; }
    // ... rest unchanged
}
```

#### 1.2 Line Caching for Components
Add caching to avoid re-parsing markdown on every frame:

**File: `src/tui_components.rs`** - Add to `ChatMessage`:
```rust
#[derive(Clone)]
pub struct ChatMessage {
    // ... existing fields
    pub cached_rendered: Option<Vec<Line<'static>>>,  // Cache for rendered lines
    pub cache_version: u64,  // Invalidate when content changes
}

// In render_message function:
fn render_message(msg: &ChatMessage, width: u16, theme: &Theme) -> Vec<Line<'static>> {
    // Check cache
    if let Some(ref cached) = msg.cached_rendered {
        if cached.len() > 0 {
            return cached.clone();
        }
    }
    
    // ... existing rendering logic
    
    // Cache result
    msg.cached_rendered = Some(result.clone());
}
```

#### 1.3 Differential Rendering (Future Enhancement)
For a more advanced implementation, track previous buffer state and only write changed lines:

```rust
// In terminal setup - store previous frame
struct TerminalState {
    previous_lines: Vec<String>,
}

fn diff_and_render(terminal: &mut Terminal, new_content: &[String]) {
    // Find first changed line
    let first_change = new_content.iter()
        .enumerate()
        .find(|(i, line)| {
            self.previous_lines.get(*i) != Some(line)
        })
        .map(|(i, _)| i);
    
    match first_change {
        Some(line_num) => {
            // Move cursor to line and rewrite from there
            execute!(terminal.backend_mut(), 
                cursor::MoveTo(0, line_num as u16))?;
            // Write new lines...
        }
        None => { /* No changes */ }
    }
}
```

### Testing
- Test in multiple terminals: Ghostty, iTerm2, VS Code terminal, Alacritty
- Verify no flicker during streaming
- Check performance with 1000+ message sessions

---

## 2. Structured Tool Results

### Problem
Current `ToolOutput` returns a single text blob. The UI has to parse it for display (e.g., diffs, statistics).

### Solution
Extend `ToolOutput` to return structured display data separately from LLM content.

### Implementation Details

**Note:** This will replace `content_for_display` entirely. All tools will be updated at once to use `structured_display`.

**File: `src/tools/mod.rs`** - Replace `ToolOutput`:

```rust
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// Content for LLM context (text that gets fed back to model)
    pub content: Arc<String>,

    /// Structured display data for rich UI rendering
    pub structured_display: Option<StructuredDisplay>,

    /// Binary attachments (images)
    pub attachments: Vec<Attachment>,

    /// Metadata about the operation
    pub metadata: OutputMetadata,
}

#[derive(Debug, Clone, Default)]
pub struct StructuredDisplay {
    /// Type of display (diff, stats, table, etc.)
    pub display_type: DisplayType,
    
    /// Key-value pairs for summary display
    pub summary: Option<HashMap<String, String>>,
    
    /// Formatted diff data
    pub diff: Option<DiffData>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayType {
    Text,
    Diff,
    Stats,      // Line counts, file sizes
    Table,
    Error,
}

#[derive(Debug, Clone, Default)]
pub struct DiffData {
    pub additions: usize,
    pub deletions: usize,
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
}
```

**File: `src/tools/impl.rs`** - Update `WriteTool`:

```rust
impl Tool for WriteTool {
    // ... existing impl
    
    fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        // ... existing write logic
        
        Ok(ToolOutput::text(content)
            .with_metadata(OutputMetadata {
                file_path: Some(path_str.clone()),
                line_count: Some(lines.len()),
                byte_count: Some(written),
                truncated: false,
                display_summary: None,
            })
            .with_structured_display(StructuredDisplay {
                display_type: DisplayType::Stats,
                summary: Some([
                    ("file".to_string(), path_str),
                    ("lines".to_string(), lines.len().to_string()),
                    ("bytes".to_string(), written.to_string()),
                ].into_iter().collect()),
                diff: None,
            }))
        )
    }
}
```

**File: `src/tools/impl.rs`** - Update `EditTool`:

```rust
impl Tool for EditTool {
    // ... existing impl
    
    fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        // ... existing edit logic
        
        Ok(ToolOutput::text(format!("Edited {} lines", changed))
            .with_structured_display(StructuredDisplay {
                display_type: DisplayType::Diff,
                summary: Some([
                    ("file".to_string(), file_path_str),
                    ("changed".to_string(), changed.to_string()),
                ].into_iter().collect()),
                diff: Some(DiffData {
                    additions: new_lines.len(),
                    deletions: old_lines.len(),
                    old_lines,
                    new_lines,
                }),
            }))
        )
    }
}
```

**File: `src/tui_components.rs`** - Update rendering:

```rust
pub fn render_tool_output(output: &ToolOutput, theme: &Theme) -> Vec<Line<'static>> {
    // Use structured display if available
    if let Some(ref structured) = output.structured_display {
        return match structured.display_type {
            DisplayType::Diff => render_diff(&structured.diff, theme),
            DisplayType::Stats => render_stats(&structured.summary, theme),
            _ => render_text(&output.content),
        };
    }
    
    // Fall back to content_for_display or content
    if let Some(ref display) = output.content_for_display {
        parse_markdown_ansi(display, theme)
    } else {
        parse_markdown_ansi(&output.content, theme)
    }
}
```

---

## 3. Type-Safe Model Registry

### Problem
Models are manually configured in codr.toml with no type safety for capabilities, costs, or features.

### Solution
Create a model registry with auto-generation from OpenRouter/models.dev.

### Implementation Details

**File: `src/model_registry.rs`** (new file):

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Anthropic,
    OpenAI,
    OpenAICompat,  // llama.cpp, Ollama, vLLM, etc.
    Google,
    xAI,
    Groq,
    Cerebras,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub vision: bool,
    pub thinking: bool,
    pub function_calling: bool,
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TokenCost {
    pub input_per_mtok: f64,   // per 1M tokens
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: Option<f64>,
    pub cache_write_per_mtok: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub provider: Provider,
    pub model_id: String,  // Provider's model ID
    
    pub context_window: u32,
    pub capabilities: ModelCapabilities,
    pub cost: TokenCost,
}

// Registry of known models
pub struct ModelRegistry {
    models: HashMap<&'static str, ModelDefinition>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        let mut registry = Self { models: HashMap::new() };
        
        // Anthropic models
        registry.register(ModelDefinition {
            id: "claude-opus-4-5-20251115",
            name: "Claude Opus 4.5",
            provider: Provider::Anthropic,
            model_id: "claude-opus-4-5-20251115".to_string(),
            context_window: 200_000,
            capabilities: ModelCapabilities {
                vision: true,
                thinking: true,
                function_calling: true,
                max_output_tokens: Some(32_000),
            },
            cost: TokenCost {
                input_per_mtok: 15.0,
                output_per_mtok: 75.0,
                cache_read_per_mtok: Some(1.5),
                cache_write_per_mtok: Some(7.5),
            },
        });
        
        // Add more models...
        
        registry
    }
    
    pub fn register(&mut self, model: ModelDefinition) {
        self.models.insert(model.id, model);
    }
    
    pub fn get(&self, id: &str) -> Option<&ModelDefinition> {
        self.models.get(id)
    }
    
    pub fn by_provider(&self, provider: Provider) -> Vec<&ModelDefinition> {
        self.models.values()
            .filter(|m| m.provider == provider)
            .collect()
    }
}

// Easy custom model definition (for codr.toml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomModelConfig {
    pub model_id: String,
    pub provider: Provider,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub context_window: Option<u32>,
}
```

**File: `src/config.rs`** - Update to use registry:

```rust
impl Config {
    pub fn load_model(&self) -> Model {
        // If model ID is in registry, use registry definition
        if let Some(def) = ModelRegistry::new().get(&self.model) {
            return Model::from_definition(def, &self.api);
        }
        
        // Fall back to config-based model (backwards compatible)
        Model::from_config(&self.model, &self.api)
    }
}
```

---

## 4. Cross-Provider Context Handoff

### Problem
Switching models mid-session loses context because providers have different message formats.

### Solution
Implement context serialization that converts between provider formats.

### Implementation Details

**File: `src/model.rs`** - Add context handoff:

```rust
impl Model {
    /// Convert messages to target provider format
    pub fn handoff_messages(
        messages: &[Message],
        target: &ModelType,
    ) -> Vec<Message> {
        let mut converted = Vec::new();
        
        for msg in messages {
            let converted_msg = match (&msg.role.as_ref(), target) {
                // Convert thinking to content block for non-thinking providers
                (Role::Assistant, ModelType::OpenAI { .. }) => {
                    if let Some(thinking) = &msg.thinking {
                        // Convert thinking trace to content block
                        let new_content = format!(
                            "<thinking>{}</thinking>\n\n{}",
                            thinking,
                            msg.content.as_ref()
                        );
                        Message {
                            role: msg.role.clone(),
                            content: Arc::new(new_content),
                            images: msg.images.clone(),
                        }
                    } else {
                        msg.clone()
                    }
                }
                // No conversion needed
                _ => msg.clone(),
            };
            converted.push(converted_msg);
        }
        
        converted
    }
    
    /// Serialize context for persistence
    pub fn serialize_context(messages: &[Message]) -> String {
        serde_json::to_string(messages).unwrap()
    }
    
    /// Deserialize and restore context
    pub fn deserialize_context(data: &str) -> Vec<Message> {
        serde_json::from_str(data).unwrap()
    }
}
```

---

## 5. Streaming Tool Results

### Problem
Bash tool returns complete output at once - no progress for long-running commands.

### Solution
Add async streaming support for bash tool output.

### Implementation Details

**File: `src/tools/impl.rs`** - Extend `BashTool`:

```rust
impl Tool for BashTool {
    fn name(&self) -> &str { "bash" }
    
    // ... existing sync execute
    
    /// Streaming execution for long-running commands
    async fn execute_streaming<F>(
        &self,
        params: Value,
        ctx: &ToolContext,
        mut on_output: F,
    ) -> Result<ToolOutput, ToolError>
    where
        F: FnMut(String) + Send,
    {
        let command = params.get_required_str("command")?;
        let timeout = params.get_str("timeout")?
            .and_then(|s| s.parse::<u64>().ok());
        
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
           .arg(&command)
           .current_dir(&ctx.cwd)
           .envs(ctx.env.iter().map(|(k,v)| (k.as_str(), v.as_str())));
        
        let mut child = cmd.spawn()?;
        
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        
        // Stream output as it comes
        // ... implementation with tokio::io::copy
        
        let status = if let Some(timeout) = timeout {
            tokio::time::timeout(Duration::from_secs(timeout), &mut child).await?
        } else {
            child.wait().await
        };
        
        // Return final output
        Ok(ToolOutput::text(final_output))
    }
}
```

---

## 6. Minimal Prompt Refinement

### Problem
Multiple prompt styles add complexity; could simplify to one minimal prompt.

### Solution
Adopt pi's minimal ~1000 token prompt as single default.

### Implementation Details

**File: `src/prompt.rs`** - Replace with minimal prompt:

```rust
/// Minimal system prompt - similar to pi agent
pub fn build_system_prompt(tools_description: &str, project_context: &str) -> String {
    let tools_section = format_tools(tools_description);
    let context_section = if project_context.is_empty() {
        String::new()
    } else {
        format!("## Project Context\n\n{project_context}\n\n")
    };
    
    format!(
r#"You are an expert coding assistant. You help users with coding tasks by \
reading files, executing commands, editing code, and writing new files.

Available tools:
{tools_section}
{context_section}
## Guidelines

- Use bash for file operations like ls, grep, find
- Use read to examine files before editing
- Use edit for precise changes (old text must match exactly)
- Use write only for new files or complete rewrites
- When summarizing your actions, output plain text directly - do NOT use cat or bash to display what you did
- Be concise in your responses
- Show file paths clearly when working with files
"#
    )
}
```

---

## 7. Context Management & Pruning

### Problem
Message history grows indefinitely. Long conversations can exceed context windows and degrade performance.

### Solution
Implement context manager with token estimation and intelligent pruning.

### Implementation Details

**File: `src/context_manager.rs`** (new file):

```rust
use crate::model::Message;
use crate::model::ModelType;

pub struct ContextManager {
    max_tokens: usize,
    current_tokens: usize,
    messages: Vec<Message>,
    system_prompt_tokens: usize,
}

impl ContextManager {
    pub fn new(max_tokens: usize, system_prompt: &str) -> Self {
        let system_prompt_tokens = estimate_tokens(system_prompt);
        Self {
            max_tokens,
            current_tokens: system_prompt_tokens,
            messages: Vec::new(),
            system_prompt_tokens,
        }
    }

    /// Add message and update token count
    pub fn add_message(&mut self, msg: Message) {
        let tokens = estimate_tokens(&msg.content);
        self.current_tokens += tokens;
        self.messages.push(msg);
    }

    /// Prune old messages while preserving important context
    pub fn prune_to_fit(&mut self, reserve: usize) {
        let target = self.max_tokens - reserve;
        if self.current_tokens <= target {
            return;
        }

        // Always keep system prompt and last 5 messages
        let keep_recent = 5;
        let mut tokens_to_remove = self.current_tokens - target;
        let mut remove_count = 0;

        // Remove from middle (keep beginning for context, end for continuity)
        let start = 1; // Skip system prompt
        let end = self.messages.len().saturating_sub(keep_recent);

        for i in start..end {
            if tokens_to_remove == 0 {
                break;
            }
            let msg_tokens = estimate_tokens(&self.messages[i].content);
            if msg_tokens <= tokens_to_remove {
                tokens_to_remove -= msg_tokens;
                remove_count += 1;
            } else {
                break;
            }
        }

        // Remove messages and update token count
        for _ in 0..remove_count {
            if let Some(msg) = self.messages.get(1) {
                self.current_tokens -= estimate_tokens(&msg.content);
                self.messages.remove(1);
            }
        }
    }

    /// Summarize old conversation to compress context
    pub async fn summarize_old_messages(
        &mut self,
        model: &crate::model::Model,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.messages.len() < 10 {
            return Ok(());
        }

        // Take first half of messages (excluding system prompt)
        let old_count = self.messages.len() / 2;
        let to_summarize: Vec<_> = self.messages
            .iter()
            .take(old_count)
            .skip(1) // Skip system prompt
            .cloned()
            .collect();

        let summary_prompt = format!(
            "Summarize this conversation concisely:\n\n{}",
            to_summarize
                .iter()
                .map(|m| format!("{}: {}", m.role, m.content))
                .collect::<Vec<_>>()
                .join("\n\n")
        );

        let summary = model.query(&[
            crate::model::Message {
                role: "user".into(),
                content: std::sync::Arc::new(summary_prompt),
                images: vec![],
            }
        ]).await?;

        // Replace old messages with summary
        let summary_msg = Message {
            role: "system".into(),
            content: std::sync::Arc::new(format!("Conversation summary:\n{}", summary)),
            images: vec![],
        };

        // Remove old messages and insert summary
        for _ in 1..old_count {
            self.messages.remove(1);
        }
        self.messages.insert(1, summary_msg);

        // Recalculate tokens
        self.current_tokens = self.system_prompt_tokens +
            self.messages.iter().map(|m| estimate_tokens(&m.content)).sum::<usize>();

        Ok(())
    }

    pub fn get_messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn token_usage(&self) -> f64 {
        (self.current_tokens as f64) / (self.max_tokens as f64)
    }
}

/// Simple token estimation (roughly 4 chars per token)
fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
}
```

**File: `src/tui.rs`** - Add to App struct:

```rust
struct App {
    // ... existing fields
    context_manager: ContextManager,
    auto_summarize: bool,  // Config option
}

impl App {
    fn check_context_usage(&mut self) {
        if self.context_manager.token_usage() > 0.9 && self.auto_summarize {
            // Trigger summarization
        }
    }
}
```

---

## 8. Syntax Highlighting for Code Blocks

### Problem
Code blocks are rendered as plain text, reducing readability.

### Solution
Add syntect-based syntax highlighting for code blocks.

### Implementation Details

**File: `Cargo.toml`** - Add dependencies:

```toml
[dependencies]
syntect = "5.2"
```

**File: `src/tui_components.rs`** - Add highlighting:

```rust
use syntect::{
    parsing::SyntaxSet,
    highlighting::{ThemeSet, Theme},
    easy::HighlightLines,
    util::LinesWithEndings,
};

pub struct Highlighter {
    ps: SyntaxSet,
    ts: ThemeSet,
    theme: Theme,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            ps: SyntaxSet::load_defaults_newlines(),
            ts: ThemeSet::load_defaults(),
            theme: ThemeSet::load_defaults().themes["base16-ocean.dark"].clone(),
        }
    }

    pub fn highlight_code(&self, code: &str, lang: &str) -> Vec<Line<'static>> {
        let syntax = self.ps
            .find_syntax_by_token(lang)
            .unwrap_or_else(|| self.ps.find_syntax_plain_text());

        let mut h = HighlightLines::new(syntax, &self.theme);

        let mut result = Vec::new();
        for line in LinesWithEndings::from(code) {
            let ranges: Vec<(syntect::highlighting::Style, &str)> =
                h.highlight_line(line, &self.ps).unwrap();

            let spans: Vec<Span> = ranges
                .into_iter()
                .map(|(style, text)| {
                    let fg = Color::Rgb(
                        style.foreground.r,
                        style.foreground.g,
                        style.foreground.b,
                    );
                    Span::styled(text, Style::default().fg(fg))
                })
                .collect();

            result.push(Line::from(spans));
        }

        result
    }
}
```

---

## 9. Session Persistence & Export

### Problem
Conversations are lost on exit. No way to resume or export.

### Solution
Add session save/load and export functionality.

### Implementation Details

**File: `src/session.rs`** (new file):

```rust
use serde::{Serialize, Deserialize};
use std::path::PathBuf;
use crate::model::{Message, ModelType};
use crate::tools::Role;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub messages: Vec<Message>,
    pub role: Role,
    pub model_type: ModelType,
    pub cwd: PathBuf,
}

impl Session {
    pub fn new(role: Role, model_type: ModelType, cwd: PathBuf) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: chrono::Utc::now(),
            messages: Vec::new(),
            role,
            model_type,
            cwd,
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let session_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("codr")
            .join("sessions");

        std::fs::create_dir_all(&session_dir)?;

        let path = session_dir.join(format!("{}.json", self.id));
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;

        Ok(())
    }

    pub fn load(id: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let session_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("codr")
            .join("sessions");

        let path = session_dir.join(format!("{}.json", id));
        let json = std::fs::read_to_string(&path)?;
        let session: Session = serde_json::from_str(&json)?;

        Ok(session)
    }

    pub fn list_sessions() -> Result<Vec<SessionMetadata>, Box<dyn std::error::Error>> {
        let session_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("codr")
            .join("sessions");

        let mut sessions = Vec::new();
        for entry in std::fs::read_dir(session_dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) == Some("json") {
                let json = std::fs::read_to_string(entry.path())?;
                let session: Session = serde_json::from_str(&json)?;
                sessions.push(SessionMetadata {
                    id: session.id,
                    created_at: session.created_at,
                    message_count: session.messages.len(),
                });
            }
        }

        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(sessions)
    }

    /// Export conversation to markdown
    pub fn export_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str(&format!("# codr Session: {}\n\n", self.id));
        md.push_str(&format!("**Created:** {}\n\n", self.created_at));
        md.push_str(&format!("**Model:** {:?}\n\n", self.model_type));
        md.push_str(&format!("**Role:** {}\n\n", self.role.name()));
        md.push_str("---\n\n");

        for msg in &self.messages {
            md.push_str(&format!("## {}\n\n", msg.role));
            md.push_str(&msg.content);
            md.push_str("\n\n");
        }

        md
    }

    /// Export conversation to JSON
    pub fn export_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
}
```

**File: `src/commands.rs`** - Add export command:

```rust
pub async fn cmd_export(app: &mut App, args: Option<&str>) -> Result<(), CommandError> {
    let format = args.unwrap_or("markdown");

    let session = Session {
        id: uuid::Uuid::new_v4().to_string(),
        created_at: chrono::Utc::now(),
        messages: app.messages.iter()
            .filter_map(|m| {
                // Convert ChatMessage to Message
                // ... conversion logic
            })
            .collect(),
        role: app.role,
        model_type: app.model.config.model_type.clone(),
        cwd: app.cwd.clone(),
    };

    let output = match format {
        "markdown" | "md" => session.export_markdown(),
        "json" => session.export_json(),
        _ => return Err(CommandError::InvalidArgument(format)),
    };

    // Write to clipboard or file
    println!("{}", output);
    Ok(())
}
```

---

## 10. Async Tool Execution Integration

### Problem
Existing `async_wrapper.rs` and `async_handler.rs` files are not integrated. Current execution uses threads.

### Solution
Integrate async tool system for better parallel execution and streaming support.

### Implementation Details

**File: `src/tools/async_handler.rs`** - Update for integration:

```rust
use tokio::task::JoinSet;
use crate::tools::{Tool, ToolContext, ToolOutput, ToolError, ToolCategory};

pub struct AsyncToolExecutor {
    cwd: std::path::PathBuf,
}

impl AsyncToolExecutor {
    pub fn new(cwd: std::path::PathBuf) -> Self {
        Self { cwd }
    }

    /// Execute multiple tools in parallel where safe
    pub async fn execute_parallel(
        &self,
        actions: Vec<(String, serde_json::Value)>,
        registry: &crate::tools::ToolRegistry,
    ) -> Vec<Result<ToolOutput, ToolError>> {
        let mut results = Vec::new();
        let mut join_set: JoinSet<(usize, Result<ToolOutput, ToolError>)> = JoinSet::new();

        // Separate read-only and write actions
        let mut read_tasks: Vec<(usize, String, serde_json::Value)> = Vec::new();
        let mut write_tasks: Vec<(usize, String, serde_json::Value)> = Vec::new();

        for (i, (name, params)) in actions.into_iter().enumerate() {
            if let Some(tool) = registry.get(&name) {
                if matches!(tool.category(), ToolCategory::FileOps | ToolCategory::Search) {
                    read_tasks.push((i, name, params));
                } else {
                    write_tasks.push((i, name, params));
                }
            }
        }

        // Spawn read-only tasks in parallel
        for (idx, name, params) in read_tasks {
            let cwd = self.cwd.clone();
            let tool_name = name.clone();

            join_set.spawn(async move {
                let ctx = ToolContext::new(cwd);
                let result = registry.execute(&tool_name, params);
                (idx, result)
            });
        }

        // Collect results
        while let Some(result) = join_set.join_next().await {
            results.push(result.unwrap());
        }

        // Execute write tasks sequentially
        for (idx, name, params) in write_tasks {
            let result = registry.execute(&name, params);
            results.push((idx, result));
        }

        // Sort by index and return
        results.sort_by_key(|(idx, _)| *idx);
        results.into_iter().map(|(_, r)| r).collect()
    }

    /// Stream bash command output
    pub async fn stream_bash<F>(
        &self,
        command: &str,
        cwd: &std::path::PathBuf,
        mut on_output: F,
    ) -> Result<ToolOutput, ToolError>
    where
        F: FnMut(String) + Send + 'static,
    {
        use tokio::process::Command;
        use tokio::io::{AsyncBufReadExt, BufReader};

        let mut cmd = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let stdout = cmd.stdout.take().unwrap();
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        let mut output = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            let line_str = line + "\n";
            on_output(line_str.clone());
            output.push_str(&line_str);
        }

        let status = cmd.await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if !status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "Command exited with status: {:?}",
                status.code()
            )));
        }

        Ok(ToolOutput::text(output))
    }
}
```

**File: `src/tui.rs`** - Integrate async executor:

```rust
struct App {
    // ... existing fields
    async_executor: Arc<AsyncToolExecutor>,
}

impl App {
    async fn execute_tools_async(
        &self,
        actions: Vec<(String, serde_json::Value)>,
    ) -> Vec<Result<ToolOutput, ToolError>> {
        self.async_executor.execute_parallel(actions, &self.tools).await
    }
}
```

---

## 11. Tool Approval UI Enhancements

### Problem
Current approval system only shows raw commands/edits without preview.

### Solution
Add diff preview and command explanation.

### Implementation Details

**File: `src/tui.rs`** - Enhanced approval UI:

```rust
pub enum ApprovalAction {
    Approve,
    Reject,
    Edit(String),  // Modify before executing
    DryRun,        // Show what would happen
}

pub struct ApprovalPrompt {
    pub action_type: ApprovalActionType,
    pub preview: Option<String>,  // Diff preview, command explanation, etc.
    pub can_edit: bool,
}

pub enum ApprovalActionType {
    Bash(String),
    Edit { file: String, old_lines: Vec<String>, new_lines: Vec<String> },
    Write { file: String, content: String },
}

impl ApprovalPrompt {
    pub fn render_preview(&self, width: u16) -> Vec<Line<'static>> {
        match &self.action_type {
            ApprovalActionType::Bash(cmd) => {
                // Show command explanation
                vec![
                    Line::from("Command to execute:".bold()),
                    Line::from(format!("$ {}", cmd).yellow()),
                ]
            }
            ApprovalActionType::Edit { file, old_lines, new_lines } => {
                // Show side-by-side diff preview
                render_diff_preview(old_lines, new_lines, width)
            }
            ApprovalActionType::Write { file, content } => {
                vec![
                    Line::from(format!("Write to: {}", file).bold()),
                    Line::from(format!("{} lines", content.lines().count()).dim()),
                ]
            }
        }
    }
}
```

---

## 12. Model Capability Auto-Detection

### Problem
Model registry relies on hardcoded capability data.

### Solution
Add runtime capability detection via probe queries.

### Implementation Details

**File: `src/model.rs`** - Add capability detection:

```rust
impl Model {
    /// Probe model capabilities via test query
    pub async fn detect_capabilities(&self) -> ModelCapabilities {
        let mut capabilities = ModelCapabilities {
            vision: false,
            thinking: false,
            function_calling: false,
            max_output_tokens: None,
        };

        // Test vision support
        let vision_test = self.query(&[
            Message {
                role: "user".into(),
                content: std::sync::Arc::new(
                    "Describe what you see in this image: iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".to_string()
                ),
                images: vec![
                    ImageAttachment {
                        data: std::sync::Arc::new(
                            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".to_string()
                        ),
                        media_type: "image/png".into(),
                    }
                ],
            }
        ]).await;

        capabilities.vision = vision_test.is_ok();

        // Test thinking support (Anthropic-specific)
        if matches!(self.config.model_type, ModelType::Anthropic) {
            capabilities.thinking = true;
        }

        // Test function calling
        let function_test = self.query_with_tools(
            &[Message {
                role: "user".into(),
                content: std::sync::Arc::new("What's 2+2? Use the calculator tool.".to_string()),
                images: vec![],
            }],
            &[],  // Dummy tools
        ).await;

        capabilities.function_calling = function_test.is_ok();

        capabilities
    }
}
```

---

## Required Dependencies

Add to `Cargo.toml`:

```toml
[dependencies]
# Existing dependencies...

# New dependencies for improvements
syntect = "5.2"           # Syntax highlighting
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.0", features = ["serde", "v4"] }
tokio = { version = "1.0", features = ["full"] }  # Already present, ensure "process" feature

# For model registry (optional - for fetching model data)
reqwest = { version = "0.12", features = ["json"] }
```

---

## Clarifications & Design Decisions

### Structured Display Approach
**Decision:** "Break and fix" - Replace `content_for_display` entirely with `structured_display`.

**Rationale:**
- Maintains single source of truth for display data
- Avoids confusion about which field takes precedence
- Forces complete migration, not partial compatibility
- Simplifies rendering logic in TUI

**Migration Path:**
1. Update `ToolOutput` struct definition
2. Add builder method `.with_structured_display()`
3. Update all tools in `src/tools/impl.rs` to use new API
4. Update TUI rendering to use `structured_display` only
5. Remove old `content_for_display` references

**Detailed Migration Plan:** See Appendix A: StructuredDisplay Migration Plan

### Synchronized Output Compatibility
**Issue:** The escape sequence `\x1b[?2026h` is not universally supported.

**Solution:** Implement fallback mechanism:

```rust
// In tui.rs
fn enable_synchronized_output(stdout: &mut io::Stdout) -> io::Result<bool> {
    match execute!(stdout, crossterm::terminal::EnableSynchronizedOutput) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),  // Gracefully degrade
    }
}

// In render loop
let sync_supported = enable_synchronized_output(&mut io::stdout())?;
terminal.draw(|f| draw_ui(f, app))?;
if sync_supported {
    disable_synchronized_output(&mut io::stdout())?;
}
```

### Async vs Thread Execution
**Decision:** Migrate to tokio-based async execution.

**Rationale:**
- Better for streaming I/O operations
- More efficient concurrent task management
- Cleaner integration with async APIs (web requests, etc.)
- `tokio` already a dependency for other features

**Migration Path:**
1. Keep existing tool trait synchronous (simple tools don't change)
2. Add async execution layer in `AsyncToolExecutor`
3. Use tokio for bash streaming and parallel tool execution
4. Background agent loop already async - minimal changes needed

### Model Cost Configuration
**Issue:** Token costs in examples are hardcoded and may become outdated.

**Solution:** Make costs configurable and updateable:

```toml
# codr.toml
[model_costs]
claude-opus-4-5 = { input = 15.0, output = 75.0, cache_read = 1.5, cache_write = 7.5 }
claude-sonnet-4-5 = { input = 3.0, output = 15.0, cache_read = 0.30, cache_write = 1.50 }

# Allow custom models
[model_costs.custom]
local-llama = { input = 0.0, output = 0.0 }
```

### Minimal Prompt Format
**Current Format:** Tool descriptions are formatted by category with headers.

**New Format:** Simplified inline descriptions:

```rust
fn format_tools(tools: &str) -> String {
    // tools is already formatted by ToolRegistry::descriptions()
    // Just pass through
    tools.to_string()
}
```

---

## Implementation Priority & Effort (Updated)

| Feature | Effort | Impact | Priority |
|---------|--------|--------|----------|
| Flicker-free TUI | Medium | High | 1 |
| Structured tool results | Medium | Medium | 2 |
| Context management | High | High | 3 |
| Async tool integration | High | Medium | 4 |
| Session persistence | Medium | Medium | 5 |
| Syntax highlighting | Medium | Medium | 6 |
| Streaming bash | Medium | Medium | 7 |
| Minimal prompt | Low | Low | 8 |
| Model registry | Medium | Medium | 9 |
| Context handoff | High | Medium | 10 |
| Approval UI enhancements | Low | Low | 11 |
| Model capability detection | Medium | Low | 12 |

---

## Additional Notes from pi Agent Research

### Key Philosophy Differences

1. **YOLO by default**: pi runs with full filesystem access and no permission prompts
   - codr already has this with YOLO mode
   
2. **No built-in to-dos**: Use external files (TODO.md, PLAN.md)
   - Already optional in codr
   
3. **No plan mode**: Use PLAN.md files instead
   - Already available in codr
   
4. **No MCP support**: Prefer CLI tools with READMEs (progressive disclosure)
   - Could consider simplifying MCP integration
   
5. **No background bash**: Use tmux instead
   - User already has full shell access
   
6. **No sub-agents**: Spawn via bash if needed
   - Could simplify architecture

### Benchmark Results

pi achieved competitive results on Terminal-Bench 2.0 with a minimal approach:
- Used only 4 tools (read, write, edit, bash)
- Minimal ~1000 token system prompt
- Full YOLO mode
- No complex features

This suggests codr's current minimal toolset is already well-suited for the task.

---

## Summary

The improvements to focus on, in priority order:

**Phase 1: Core UX (High Impact)**
1. **Flicker-free TUI** - Addresses visible user pain point
2. **Structured tool results** - Enables richer UI for diffs, stats
3. **Context management** - Prevents long conversation issues

**Phase 2: Feature Enhancements**
4. **Async tool integration** - Better parallel execution and streaming
5. **Session persistence** - Save/resume conversations
6. **Syntax highlighting** - Improved code readability
7. **Streaming bash** - Better UX for long commands

**Phase 3: Developer Experience**
8. **Model registry** - Improves DX for custom models
9. **Context handoff** - Enables flexible model switching
10. **Minimal prompt** - Simplifies maintenance

**Phase 4: Polish (Optional)**
11. **Approval UI enhancements** - Better preview experience
12. **Model capability detection** - Automatic model feature detection

### Implementation Order Recommendation

Start with **#1 (Flicker-free TUI)** for immediate user impact, then:
- Implement #2-3 to solidify core functionality
- Add #4-6 for feature completeness
- Complete #7-10 for polish and DX improvements
- Consider #11-12 as nice-to-have enhancements

### Phased Rollout Strategy

**Alpha Release:** Items 1-3
**Beta Release:** Items 4-7
**Stable Release:** Items 8-10
**Future:** Items 11-12

---

## Appendix A: StructuredDisplay Migration Plan

This document provides a detailed, step-by-step migration plan for replacing `content_for_display` with `structured_display` across the codebase.

### Overview

**Current State:**
```rust
pub struct ToolOutput {
    pub content: Arc<String>,
    pub content_for_display: Option<Arc<String>>,  // TO BE REMOVED
    pub attachments: Vec<Attachment>,
    pub metadata: OutputMetadata,
}
```

**Target State:**
```rust
pub struct ToolOutput {
    pub content: Arc<String>,
    pub structured_display: Option<StructuredDisplay>,  // NEW
    pub attachments: Vec<Attachment>,
    pub metadata: OutputMetadata,
}
```

### Migration Phases

---

### Phase 1: Preparation (Non-Breaking)

**Goal:** Add new types and methods without breaking existing code.

#### Step 1.1: Add new types to `src/tools/mod.rs`

```rust
// Add these after the Attachment definition

#[derive(Debug, Clone, Default)]
pub struct StructuredDisplay {
    /// Type of display (diff, stats, table, etc.)
    pub display_type: DisplayType,

    /// Key-value pairs for summary display
    pub summary: Option<HashMap<String, String>>,

    /// Formatted diff data
    pub diff: Option<DiffData>,

    /// Table data for tabular output
    pub table: Option<TableData>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayType {
    Text,        // Plain text (default fallback)
    Diff,        // Side-by-side or unified diff
    Stats,       // Statistics (line counts, file sizes, etc.)
    Table,       // Tabular data
    Error,       // Error message with formatting
    Progress,    // Progress indicator for long-running operations
}

#[derive(Debug, Clone, Default)]
pub struct DiffData {
    pub additions: usize,
    pub deletions: usize,
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TableData {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub alignments: Vec<TableAlignment>,  // Left, Center, Right
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAlignment {
    Left,
    Center,
    Right,
}
```

#### Step 1.2: Add builder methods to `ToolOutput`

```rust
impl ToolOutput {
    // ... existing methods: text(), with_attachment(), with_metadata(), with_summary_display()

    /// NEW: Add structured display data
    pub fn with_structured_display(mut self, display: StructuredDisplay) -> Self {
        self.structured_display = Some(display);
        self
    }

    /// NEW: Convenience method for diff display
    pub fn with_diff(
        mut self,
        file_path: String,
        old_lines: Vec<String>,
        new_lines: Vec<String>,
    ) -> Self {
        let additions = new_lines.len();
        let deletions = old_lines.len();

        self.structured_display = Some(StructuredDisplay {
            display_type: DisplayType::Diff,
            summary: Some([
                ("file".to_string(), file_path.clone()),
                ("additions".to_string(), additions.to_string()),
                ("deletions".to_string(), deletions.to_string()),
            ].into_iter().collect()),
            diff: Some(DiffData {
                additions,
                deletions,
                old_lines,
                new_lines,
                file_path: Some(file_path),
            }),
            ..Default::default()
        });
        self
    }

    /// NEW: Convenience method for stats display
    pub fn with_stats(mut self, stats: HashMap<String, String>) -> Self {
        self.structured_display = Some(StructuredDisplay {
            display_type: DisplayType::Stats,
            summary: Some(stats),
            ..Default::default()
        });
        self
    }

    /// NEW: Convenience method for error display
    pub fn with_error(mut self, message: String) -> Self {
        self.structured_display = Some(StructuredDisplay {
            display_type: DisplayType::Error,
            summary: Some([("error".to_string(), message)].into_iter().collect()),
            ..Default::default()
        });
        self
    }

    /// TEMPORARY: Bridge method to migrate from content_for_display
    /// This will be removed after migration is complete
    #[deprecated(note = "Use with_structured_display() instead")]
    pub fn with_summary_display<S: Into<Arc<String>>>(mut self, summary: S) -> Self {
        // For now, convert to a simple Text display type
        let summary_str = summary.into();
        self.structured_display = Some(StructuredDisplay {
            display_type: DisplayType::Text,
            summary: None,
            diff: None,
            table: None,
        });
        // Keep both during migration
        self.content_for_display = Some(summary_str);
        self
    }
}
```

#### Step 1.3: Add rendering functions to `src/tui_components.rs`

```rust
use crate::tools::{StructuredDisplay, DisplayType, DiffData, TableData, TableAlignment};

/// Render structured display data to TUI lines
pub fn render_structured_display(
    display: &StructuredDisplay,
    theme: &Theme,
) -> Vec<Line<'static>> {
    match display.display_type {
        DisplayType::Text => {
            // Fall back to rendering content
            vec![Line::from("Text output".dim())]
        }
        DisplayType::Diff => {
            if let Some(ref diff) = display.diff {
                render_diff_display(diff, theme)
            } else {
                vec![Line::from("Diff data missing".red())]
            }
        }
        DisplayType::Stats => {
            if let Some(ref summary) = display.summary {
                render_stats_display(summary, theme)
            } else {
                vec![]
            }
        }
        DisplayType::Table => {
            if let Some(ref table) = display.table {
                render_table_display(table, theme)
            } else {
                vec![Line::from("Table data missing".red())]
            }
        }
        DisplayType::Error => {
            if let Some(ref summary) = display.summary {
                if let Some(msg) = summary.get("error") {
                    vec![
                        Line::from("Error:".red().bold()),
                        Line::from(msg.clone().red()),
                    ]
                } else {
                    vec![Line::from("Unknown error".red())]
                }
            } else {
                vec![Line::from("Error".red())]
            }
        }
        DisplayType::Progress => {
            render_progress_display(display, theme)
        }
    }
}

/// Render a diff with colored additions/deletions
fn render_diff_display(diff: &DiffData, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Header
    if let Some(ref path) = diff.file_path {
        lines.push(Line::from(format!("Diff: {}", path)).bold());
    }

    let summary = format!("+{} -{}", diff.additions, diff.deletions);
    lines.push(Line::from(summary).dim());

    lines.push(Line::default());

    // Unified diff style
    for old in &diff.old_lines {
        lines.push(Line::from(format!("- {}", old)).fg(theme.diff_remove));
    }

    for new in &diff.new_lines {
        lines.push(Line::from(format!("+ {}", new)).fg(theme.diff_add));
    }

    lines
}

/// Render statistics as key-value pairs
fn render_stats_display(summary: &HashMap<String, String>, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Calculate max key width for alignment
    let max_width = summary.keys().map(|k| k.len()).max().unwrap_or(0);

    for (key, value) in summary {
        let padded_key = format!("{:width$}", key, width = max_width);
        lines.push(Line::from(vec![
            Span::styled(format!("{}: ", padded_key), Style::default().fg(theme.key_color)),
            Span::styled(value.clone(), Style::default().fg(theme.value_color)),
        ]));
    }

    lines
}

/// Render table data
fn render_table_display(table: &TableData, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Calculate column widths
    let mut col_widths: Vec<usize> = table
        .headers
        .iter()
        .map(|h| h.len())
        .collect();

    for row in &table.rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_widths.len() {
                col_widths[i] = col_widths[i].max(cell.len());
            }
        }
    }

    // Header row
    let header_spans: Vec<Span> = table
        .headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            Span::styled(
                format!("{:width$}", h, width = col_widths[i]),
                Style::default().bold().fg(theme.table_header),
            )
        })
        .collect();

    lines.push(Line::from(header_spans));
    lines.push(Line::from(
        "─".repeat(col_widths.iter().sum::<usize>() + col_widths.len() - 1)
    ));

    // Data rows
    for row in &table.rows {
        let spans: Vec<Span> = row
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                Span::styled(
                    format!("{:width$}", cell, width = col_widths[i]),
                    Style::default().fg(theme.table_text),
                )
            })
            .collect();

        lines.push(Line::from(spans));
    }

    lines
}

/// Render progress indicator
fn render_progress_display(display: &StructuredDisplay, theme: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::from("Processing...".dim()),
        Line::from("▓▓▓▓▓▓▓▓░░░".fg(theme.progress_bar)),
    ]
}
```

#### Step 1.4: Update `render_tool_output` to support both (temporary bridge)

```rust
pub fn render_tool_output(output: &ToolOutput, theme: &Theme) -> Vec<Line<'static>> {
    // NEW: Try structured display first
    if let Some(ref structured) = output.structured_display {
        return render_structured_display(structured, theme);
    }

    // FALLBACK: Use content_for_display during migration
    if let Some(ref display) = output.content_for_display {
        parse_markdown_ansi(display, theme)
    } else {
        // ULTIMATE FALLBACK: Raw content
        parse_markdown_ansi(&output.content, theme)
    }
}
```

**Deliverable:** Code compiles, all existing functionality works, new infrastructure is in place but unused.

---

### Phase 2: Tool Migration (Breaking Changes)

**Goal:** Update each tool to use the new API.

#### Step 2.1: Update `WriteTool` in `src/tools/impl.rs`

```rust
impl Tool for WriteTool {
    fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let path = params.get_str("file_path")?.ok_or_else(|| {
            ToolError::InvalidParameters("Missing file_path".to_string())
        })?;

        let content = params.get_str("content")?.ok_or_else(|| {
            ToolError::InvalidParameters("Missing content".to_string())
        })?;

        let full_path = ctx.resolve_path(&path);

        // Create parent directories if needed
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&full_path, content)?;

        let lines = content.lines().count();
        let bytes = content.len();

        Ok(ToolOutput::text(format!("Wrote {} lines to {}", lines, path))
            .with_metadata(OutputMetadata {
                file_path: Some(path.clone()),
                line_count: Some(lines),
                byte_count: Some(bytes),
                truncated: false,
                display_summary: None,
            })
            // NEW: Use structured display
            .with_stats([
                ("file".to_string(), path),
                ("lines".to_string(), lines.to_string()),
                ("bytes".to_string(), to_human_size(bytes)),
                ("action".to_string(), "wrote".to_string()),
            ].into_iter().collect())
        )
    }
}

// Helper function
fn to_human_size(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = 1024 * KB;
    const GB: usize = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
```

#### Step 2.2: Update `EditTool` in `src/tools/impl.rs`

```rust
impl Tool for EditTool {
    fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        // Parse parameters...
        let file_path = params.get_str("file_path")?.ok_or_else(|| {
            ToolError::InvalidParameters("Missing file_path".to_string())
        })?;

        let old_text = params.get_str("old_text")?.ok_or_else(|| {
            ToolError::InvalidParameters("Missing old_text".to_string())
        })?;

        let new_text = params.get_str("new_text")?.ok_or_else(|| {
            ToolError::InvalidParameters("Missing new_text".to_string())
        })?;

        let full_path = ctx.resolve_path(&file_path);

        // Read existing content
        let existing = std::fs::read_to_string(&full_path)
            .map_err(|e| ToolError::PathNotFound(file_path.clone()))?;

        // Validate old_text exists in file
        if !existing.contains(old_text) {
            return Err(ToolError::InvalidParameters(
                "old_text not found in file".to_string()
            ));
        }

        // Perform replacement
        let new_content = existing.replacen(old_text, new_text, 1);

        // Write back
        std::fs::write(&full_path, new_content)?;

        // Calculate diff data
        let old_lines: Vec<String> = old_text.lines().map(|s| s.to_string()).collect();
        let new_lines: Vec<String> = new_text.lines().map(|s| s.to_string()).collect();

        Ok(ToolOutput::text(format!("Edited {}", file_path))
            .with_metadata(OutputMetadata {
                file_path: Some(file_path.clone()),
                line_count: Some(new_content.lines().count()),
                byte_count: Some(new_content.len()),
                truncated: false,
                display_summary: None,
            })
            // NEW: Use structured display with diff
            .with_diff(
                file_path,
                old_lines,
                new_lines,
            )
        )
    }
}
```

#### Step 2.3: Update `BashTool` in `src/tools/impl.rs`

```rust
impl Tool for BashTool {
    fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let command = params.get_str("command")?.ok_or_else(|| {
            ToolError::InvalidParameters("Missing command".to_string())
        })?;

        let timeout_secs = params.get_str("timeout")
            .and_then(|t| t.parse::<u64>().ok())
            .unwrap_or(30);

        // Execute command...
        let output = execute_bash(command, ctx, timeout_secs)?;

        let exit_code = output.status.code().unwrap_or(-1);

        if exit_code == 0 {
            Ok(ToolOutput::text(output.stdout.clone())
                .with_metadata(OutputMetadata {
                    file_path: None,
                    line_count: Some(output.stdout.lines().count()),
                    byte_count: Some(output.stdout.len()),
                    truncated: output.truncated,
                    display_summary: None,
                })
                // NEW: Add execution stats
                .with_stats([
                    ("command".to_string(), command.to_string()),
                    ("exit_code".to_string(), exit_code.to_string()),
                    ("duration_ms".to_string(), output.duration_ms.to_string()),
                ].into_iter().collect())
            )
        } else {
            // Error case - use Error display type
            Ok(ToolOutput::text(output.stderr.clone())
                .with_error(format!(
                    "Command failed with exit code {}: {}",
                    exit_code,
                    output.stderr.trim()
                ))
            )
        }
    }
}
```

#### Step 2.4: Update `ReadTool` in `src/tools/impl.rs`

```rust
impl Tool for ReadTool {
    fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let file_path = params.get_str("file_path")?.ok_or_else(|| {
            ToolError::InvalidParameters("Missing file_path".to_string())
        })?;

        let full_path = ctx.resolve_path(&file_path);

        let content = std::fs::read_to_string(&full_path)
            .map_err(|e| ToolError::PathNotFound(file_path.clone()))?;

        let lines = content.lines().count();
        let bytes = content.len();

        Ok(ToolOutput::text(content)
            .with_metadata(OutputMetadata {
                file_path: Some(file_path.clone()),
                line_count: Some(lines),
                byte_count: Some(bytes),
                truncated: false,
                display_summary: None,
            })
            // NEW: Add file info as stats
            .with_stats([
                ("file".to_string(), file_path),
                ("lines".to_string(), lines.to_string()),
                ("bytes".to_string(), to_human_size(bytes)),
            ].into_iter().collect())
        )
    }
}
```

#### Step 2.5: Update `GrepTool` and `FindTool`

For search tools, use `Table` display type:

```rust
impl Tool for GrepTool {
    fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        // ... grep logic ...

        let matches = execute_grep(pattern, &ctx.cwd)?;

        if matches.is_empty() {
            return Ok(ToolOutput::text("No matches found")
                .with_stats([("matches".to_string(), "0".to_string())].into_iter().collect())
            );
        }

        // NEW: Return as table
        let mut headers = vec!["File".to_string(), "Line".to_string(), "Content".to_string()];
        let mut rows = Vec::new();

        for m in &matches {
            rows.push(vec![
                m.file_path.clone(),
                m.line_number.to_string(),
                m.line_content.trim().to_string(),
            ]);
        }

        Ok(ToolOutput::text(format!("Found {} matches", matches.len()))
            .with_structured_display(StructuredDisplay {
                display_type: DisplayType::Table,
                summary: Some([("matches".to_string(), matches.len().to_string())].into_iter().collect()),
                diff: None,
                table: Some(TableData {
                    headers,
                    rows,
                    alignments: vec![TableAlignment::Left, TableAlignment::Right, TableAlignment::Left],
                }),
            })
        )
    }
}
```

**Deliverable:** All tools now use `structured_display`. Tests pass.

---

### Phase 3: Cleanup (Final Breaking Change)

**Goal:** Remove deprecated `content_for_display` field.

#### Step 3.1: Remove `content_for_display` from `ToolOutput`

```rust
// src/tools/mod.rs

#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// Content for LLM context (text that gets fed back to model)
    pub content: Arc<String>,

    /// Structured display data for rich UI rendering
    pub structured_display: Option<StructuredDisplay>,

    /// Binary attachments (images)
    pub attachments: Vec<Attachment>,

    /// Metadata about the operation
    pub metadata: OutputMetadata,

    // REMOVED: pub content_for_display: Option<Arc<String>>,
}
```

#### Step 3.2: Remove deprecated builder methods

```rust
impl ToolOutput {
    // REMOVE this method:
    #[deprecated(note = "Use with_structured_display() instead")]
    pub fn with_summary_display<S: Into<Arc<String>>>(mut self, summary: S) -> Self {
        // ...
    }
}
```

#### Step 3.3: Update `render_tool_output` to final version

```rust
// src/tui_components.rs

pub fn render_tool_output(output: &ToolOutput, theme: &Theme) -> Vec<Line<'static>> {
    // Use structured display if available
    if let Some(ref structured) = output.structured_display {
        return render_structured_display(structured, theme);
    }

    // Fall back to rendering content as markdown
    parse_markdown_ansi(&output.content, theme)
}
```

#### Step 3.4: Remove any remaining `content_for_display` references

```bash
# Search for any remaining references
grep -r "content_for_display" src/
```

**Deliverable:** Clean API, single source of truth for display data.

---

### Phase 4: Testing & Validation

#### Step 4.1: Add unit tests for new types

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_structured_display_stats() {
        let display = StructuredDisplay {
            display_type: DisplayType::Stats,
            summary: Some([
                ("file".to_string(), "test.txt".to_string()),
                ("lines".to_string(), "42".to_string()),
            ].into_iter().collect()),
            ..Default::default()
        };

        assert_eq!(display.display_type, DisplayType::Stats);
        assert!(display.summary.is_some());
    }

    #[test]
    fn test_tool_output_with_diff() {
        let output = ToolOutput::text("test".to_string())
            .with_diff(
                "file.txt".to_string(),
                vec!["old line".to_string()],
                vec!["new line".to_string()],
            );

        assert!(output.structured_display.is_some());
        let display = output.structured_display.unwrap();
        assert_eq!(display.display_type, DisplayType::Diff);
        assert!(display.diff.is_some());
    }

    #[test]
    fn test_render_diff_display() {
        let diff = DiffData {
            additions: 1,
            deletions: 1,
            old_lines: vec!["old".to_string()],
            new_lines: vec!["new".to_string()],
            file_path: Some("test.txt".to_string()),
        };

        let theme = Theme::default();
        let lines = render_diff_display(&diff, &theme);

        assert!(!lines.is_empty());
        // Verify content contains expected elements
    }
}
```

#### Step 4.2: Integration testing

Run full conversation scenarios to ensure:
1. Tool outputs render correctly in TUI
2. Diff displays show proper colors
3. Stats are formatted correctly
4. Tables align properly

#### Step 4.3: Performance testing

Ensure rendering performance hasn't degraded:
- Test with 1000+ message sessions
- Verify no memory leaks in cached renderings
- Check TUI responsiveness

---

### Rollback Plan

If issues arise during migration:

```rust
// Rollback: Revert to old rendering logic
pub fn render_tool_output(output: &ToolOutput, theme: &Theme) -> Vec<Line<'static>> {
    // PREFER: content_for_display if available (old behavior)
    if let Some(ref display) = output.content_for_display {
        return parse_markdown_ansi(display, theme);
    }

    // FALLBACK: structured display if available
    if let Some(ref structured) = output.structured_display {
        return render_structured_display(structured, theme);
    }

    // ULTIMATE: raw content
    parse_markdown_ansi(&output.content, theme)
}
```

---

### Migration Checklist

- [ ] Phase 1.1: Add new types to `src/tools/mod.rs`
- [ ] Phase 1.2: Add builder methods to `ToolOutput`
- [ ] Phase 1.3: Add rendering functions to `src/tui_components.rs`
- [ ] Phase 1.4: Update `render_tool_output` (bridge)
- [ ] Phase 2.1: Migrate `WriteTool`
- [ ] Phase 2.2: Migrate `EditTool`
- [ ] Phase 2.3: Migrate `BashTool`
- [ ] Phase 2.4: Migrate `ReadTool`
- [ ] Phase 2.5: Migrate `GrepTool` and `FindTool`
- [ ] Phase 3.1: Remove `content_for_display` field
- [ ] Phase 3.2: Remove deprecated builder methods
- [ ] Phase 3.3: Update `render_tool_output` (final)
- [ ] Phase 3.4: Remove all `content_for_display` references
- [ ] Phase 4.1: Add unit tests
- [ ] Phase 4.2: Integration testing
- [ ] Phase 4.3: Performance testing

---

### Estimated Timeline

| Phase | Steps | Estimated Time |
|-------|-------|----------------|
| Phase 1 | Preparation | 2-3 hours |
| Phase 2 | Tool Migration | 3-4 hours |
| Phase 3 | Cleanup | 1 hour |
| Phase 4 | Testing | 2-3 hours |
| **Total** | | **8-11 hours** |
