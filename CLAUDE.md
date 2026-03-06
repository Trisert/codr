# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Run Commands

```bash
# Build the project
cargo build

# Build release version
cargo build --release

# Run in TUI mode (interactive chat) - this is the default
cargo run --

# Run in direct mode (single task, non-interactive)
cargo run -- -d "your task here"

# Run with YOLO mode (auto-approve bash commands)
cargo run -- --yolo
```

**Note:** The project currently has no automated tests.

## Architecture Overview

codr is a terminal-based AI agent that supports multiple LLM providers (OpenAI-compatible/llama.cpp, Anthropic Claude) and uses a tool-calling approach for codebase interaction.

### Core Components

```
src/
├── main.rs          # Entry point, CLI parsing, direct mode execution
├── model.rs         # Multi-provider LLM abstraction with streaming support
├── parser.rs        # Parses codr_tool/codr_bash XML blocks and JSON tool calls
├── config.rs        # TOML configuration loader with XDG directory support
├── error.rs         # Custom error types (Timeout, Terminating)
├── agent/           # Unified agent loop implementation
│   ├── mod.rs       # Exports LoopConfig, LoopResult, ActionExecutor, etc.
│   ├── executor.rs  # ActionExecutor trait with DirectExecutor and TUIExecutor
│   ├── loop_.rs     # Core agent loop with unified streaming/non-streaming support
│   ├── tui_executor.rs # TUI-specific executor with approval workflow
│   └── updates.rs   # TuiUpdate types for agent→UI communication
├── tui/             # Modular TUI implementation
│   ├── mod.rs       # Main TUI entry point and agent loop integration
│   ├── theme.rs     # Theme configuration (colors, styles)
│   ├── markdown.rs  # Markdown renderer using pulldown-cmark
│   ├── events.rs    # Event handling (keyboard, mouse)
│   └── widgets/     # Modular widget architecture
│       ├── mod.rs   # Widget exports
│       ├── conversation.rs # Message display with markdown rendering
│       ├── input.rs # User input widget
│       ├── banner.rs # Status bar (model, role, tokens, cost)
│       └── status.rs # Toast notifications and progress
└── tools/
    ├── mod.rs       # Tool trait, registry, categories, role filtering
    ├── impl.rs      # Tool implementations (read, bash, edit, write, grep, find)
    ├── schema.rs    # JSON schema types for tool parameters
    ├── context.rs   # Tool execution context (cwd, env, limits)
    └── async_wrapper.rs # Async tool wrappers (experimental)
```

### Unified Agent Loop

The `src/agent/` module provides the core agent loop used by both direct and TUI modes:

**`agent::run_agent_loop()`**: Single function supporting both streaming and non-streaming modes via `LoopConfig`
- Executes LLM queries and action execution
- Handles retry logic (3 max retries by default)
- Supports different executor patterns (stdout, UI channels)
- Returns `LoopResult` with final response and conversation

**`agent::LoopConfig`**: Configuration for agent loop behavior
- `streaming: bool` - Enable streaming mode
- `on_streaming: Option<StreamingCallback>` - Callback for text chunks
- `on_thinking: Option<ThinkingCallback>` - Callback for thinking chunks
- `cancel_token: Option<CancellationToken>` - For cancellation

**`agent::ActionExecutor` trait**: Abstraction for action execution
- `execute_action()` - Execute a single action, return result
- `needs_approval()` - Check if action requires user approval
- `approve_action()` / `reject_action()` - Handle approval responses

**`agent::DirectExecutor`**: Command-line executor (direct mode)
- Writes output to stdout
- No approval workflow (always auto-approves)

**`agent::TUIExecutor`**: TUI executor (TUI mode)
- Sends updates via `mpsc::unbounded_channel<TuiUpdate>`
- Manages approval workflow internally
- Returns `__APPROVAL_NEEDED__` error when approval required

### TUI Architecture

The TUI uses a **modular widget architecture** with a **background agent loop**:

**Layout:**
```
┌─────────────────────────────────────┐
│ Banner (model, role, tokens, cost)  │  <- chunks[0]
├─────────────────────────────────────┤
│                                     │
│   Conversation (messages)            │  <- chunks[1]
│   - markdown rendering               │
│   - scrolling support                │
│                                     │
├─────────────────────────────────────┤
│ Input (prompt + role indicator)      │  <- chunks[2]
└─────────────────────────────────────┘
```

**Background Agent Loop:**
1. Main thread runs ratatui event loop
2. Background task (`agent_loop` in `tui/mod.rs`) handles LLM queries and tool execution
3. Channel communication sends updates via `TuiUpdate`:
   - `StreamingChunk` - Text content chunks
   - `StreamingThinkingChunk` - Thinking content chunks
   - `ActionMessage` - Tool/bash action to execute
   - `OutputMessage` - Tool execution output
   - `NeedsApproval` - Approval request for SAFE mode
   - `ErrorMessage` - Error messages
   - `UsageUpdate` - Token/cost updates
   - `Done` - Stream complete

**Key TUI state variables:**
- `streaming_content` - Accumulated text content (flushed on Done)
- `streaming_thinking` - Accumulated thinking (flushed on newlines)
- `pending_action` - Current action awaiting approval
- `agent_status` - Current agent state (Idle, Thinking, Running)

### Markdown Rendering

Assistant messages use `src/tui/markdown.rs`:
- Based on pulldown-cmark parser
- Renders to `ratatui::Text` with proper `Span` styling
- Supports: headers, code blocks, lists, blockquotes, emphasis (bold/italic), horizontal rules
- Word-aware wrapping via textwrap crate
- Code blocks are NOT wrapped (preserves formatting)
- Nested structures supported (lists in quotes, etc.)

**Important:** The markdown renderer is designed for terminal display - it truncates overly long lines rather than overflowing the viewport.

### Tool System

Tools implement the `Tool` trait and are registered in `ToolRegistry`:

```rust
trait Tool {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> &ToolSchema;
    fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError>;
}
```

**Available tools:** `read`, `bash`, `edit`, `write`, `grep`, `find`, `file_info`

**Tool Categories (for role filtering):**
- `FileOps` - read, write, edit, file_info
- `Search` - grep, find
- `System` - bash

### Role System

Three operation modes (cycled with `Shift+Tab`):
- **YOLO** (Red) - Full access, all tools auto-approved
- **SAFE** (Green, default) - All tools available, write/edit/bash require approval
- **PLAN** (Blue) - Read-only: read, bash, grep, find, file_info only

**Role filtering:** `ToolRegistry::get_tools_for_role(role)` returns filtered tool list.

### Parser

The parser (`parse_action()` in `src/parser.rs`) handles multiple formats:
- **Native tool calling** - JSON format from OpenAI/Anthropic APIs
- **XML Tool actions:** `<codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>`
- **XML Bash actions:** `<codr_bash>ls -la</codr_bash>`
- **Plain responses:** Text without tool calls (conversation end)

Also provides `clean_message_content()` for removing XML tags from content for display.

### Model Abstraction

`ModelType` enum supports:
- **OpenAI** - OpenAI-compatible API (defaults to localhost:8080 for llama.cpp)
- **Anthropic** - Claude API with extended thinking support

Each implements:
- Non-streaming `query()`
- Streaming `query_streaming()` with separate text/thinking callbacks
- Usage tracking (tokens, cost)

### Configuration

Config loading priority:
1. `./codr.toml` (current directory)
2. `~/.config/codr/config.toml` (XDG config home)
3. Default (OpenAI on localhost:8080)

**Example `codr.toml`:**
```toml
model = "openai"  # or "anthropic"

[openai]
base_url = "http://localhost:8080"
model = "default"
api_key = "..."  # optional

[anthropic]
api_key = "sk-ant-..."  # or set ANTHROPIC_API_KEY env var
```

## Important Implementation Notes

### Adding New Tools

1. Implement `Tool` trait in `src/tools/impl.rs`
2. Implement `category()` returning `ToolCategory` (FileOps/Search/System)
3. Register in `create_coding_tools()` in `src/tools/mod.rs`
4. Update `Role::tool_available()` if the tool should be restricted

### Edit Tool Modes

The `edit` tool supports two modes:
1. **String replacement:** `{"file_path": "src/main.rs", "old_text": "old", "new_text": "new"}`
2. **Line-based editing:** `{"file_path": "src/main.rs", "line_start": 10, "line_end": 20, "new_content": "new"}`

Both require reading the file first to verify contents.

### Error Handling

Two error types:
- `AgentError::Timeout` - Command timeout/retry (feeds error back to LLM)
- `AgentError::Terminating` - Fatal error (ends conversation)

### Tool Validation

Tools should validate parameters to catch malformed LLM output:
- Check for template syntax like `{pattern}`, `{file}`
- Detect incomplete JSON or unmatched braces
- Return clear error messages

### TUI Keybindings

**Input:**
- `Up/Down` - Navigate history
- `Left/Right` - Move cursor
- `Enter` - Insert newline
- `Ctrl+S` - Send message

**Role & Approval:**
- `Shift+Tab` - Cycle roles
- `[a]` - Approve pending action
- `[r]` - Reject pending action

**Navigation:**
- `Ctrl+Home/End` - Scroll to top/bottom
- `Ctrl+Up/Down` - Scroll by 3 lines
- `Page Up/Down` - Page scroll
- `Mouse wheel` - Scroll

**Other:**
- `Ctrl+C` - Cancel (first press) / Quit (second press within 2s)
- `Ctrl+Q` - Quit
- `Ctrl+O` - Copy to clipboard
- `Ctrl+Y` / `Ctrl+Shift+V` - Paste from clipboard
