# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Run Commands

```bash
# Build the project
cargo build

# Build release version
cargo build --release

# Run in TUI mode (interactive chat)
cargo run -- --chat
# or shorthand
cargo run -c

# Run in direct mode (single task, non-interactive)
cargo run -- "your task here"

# Run with YOLO mode (auto-approve bash commands)
cargo run -- --yolo --chat
```

**Note:** There are no tests in this project currently.

## Architecture Overview

codr is a terminal-based AI agent (~400 lines of core implementation) that supports multiple LLM providers (OpenAI-compatible/llama.cpp, Anthropic Claude, NVIDIA NIM) and uses a tool-calling approach for codebase interaction.

### Core Components

```
src/
├── main.rs          # Entry point, CLI parsing, direct mode execution loop
├── model.rs         # Multi-provider LLM abstraction with streaming support
├── parser.rs        # Parses tool-action and bash-action blocks from LM responses
├── config.rs        # TOML configuration loader with XDG directory support
├── error.rs         # Custom error types (Timeout, Terminating)
├── tui.rs           # Terminal UI with background agent loop pattern
├── tui_components.rs # Message rendering, markdown, themes
└── tools/
    ├── mod.rs       # Tool trait, registry, context, output types
    ├── impl.rs      # Tool implementations (read, bash, edit, write, grep, find)
    ├── schema.rs    # JSON schema types for tool parameters
    ├── context.rs   # Tool execution context (cwd, env, limits)
    └── async_handler.rs # Codex-style async tool handler system (experimental)
```

### Agent Loop Flow

The agent follows this loop in both direct and TUI modes:

1. Send conversation history to LLM
2. LLM proposes an action (tool call or bash command)
3. Parse and execute the action
4. Feed output back to LLM
5. Repeat until a plain text response or exit command

**Direct mode** (`run_direct()` in main.rs): Synchronous loop, prints to stdout
**TUI mode** (`agent_loop()` in tui.rs): Asynchronous background task with channel-based UI updates

### Streaming Implementation (Important)

The model uses **separate callbacks for thinking and text content** to support real-time progressive display:

```rust
pub async fn query_streaming<F, G>(
    &self,
    messages: &[Message],
    on_text: F,      // Called for each text chunk
    on_thinking: G,  // Called for each thinking chunk
) -> Result<String>
```

**Key implementation details:**
- **Thinking chunks** are accumulated and flushed to messages on newlines (creates natural sentence chunks)
- **Text chunks** are accumulated in real-time and shown in the streaming buffer
- **Tool-action blocks** (````tool-action` and ````bash-action`) are filtered from display via `clean_content()`
- **Cancellation** works by checking the cancel token in streaming callbacks and dropping the update channel

### TUI Architecture

The TUI uses a **background agent loop pattern**:

1. **Main thread** runs the terminal UI event loop (ratatui)
2. **Background task** (`agent_loop`) handles LLM queries and tool execution
3. **Channel communication** (`mpsc::unbounded_channel`) sends updates from background to UI:
   - `StreamingChunk` - Text content chunks
   - `StreamingThinkingChunk` - Thinking content chunks
   - `ActionMessage` - Tool/bash action to execute
   - `OutputMessage` - Tool execution output
   - `ErrorMessage` - Error messages
   - `UsageUpdate` - Token/cost updates
   - `Done` - Stream complete

**Critical state variables:**
- `streaming_content` - Accumulated text content (flushed on Done)
- `streaming_thinking` - Accumulated thinking (flushed on newlines)
- `is_streaming_thinking` - Flag tracking current stream type
- `update_rx` - Channel receiver (dropped on cancellation to stop processing)

### Tool System

Tools implement the `Tool` trait and are registered in a `ToolRegistry`:

```rust
trait Tool {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> &ToolSchema;
    fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError>;
}
```

**Tool output includes:**
- `content` - Text result
- `attachments` - Binary data (e.g., images)
- `metadata` - File path, line count, truncation status

**Available tools:** `read`, `bash`, `edit`, `write`, `grep`, `find`

**Async Tool System (Experimental):**
`src/tools/async_handler.rs` contains a Codex-style async tool handler system with:
- `AsyncToolHandler` trait for async tool execution
- `ToolInvocation` struct with conversation history context
- `AsyncToolRegistry` with parallel execution support for read-only tools
- Currently not integrated but available for future enhancement

### Parser

The parser (`parse_action()`) handles multiple action formats from different LLM providers:

- **Tool actions:** ````tool-action\n<tool_name>\n<json_params>\n````
- **Bash actions:** ````bash-action\n<command>\n``` or JSON format with workdir/timeout/env
- **Plain responses:** Text without tool calls (indicates conversation end)

The parser uses regex with `(?s)` dotall flag to capture multi-line blocks.

### Model Abstraction

`ModelType` enum supports multiple providers:
- **OpenAI** - OpenAI-compatible API (defaults to local llama.cpp at localhost:8080)
- **Anthropic** - Claude API with extended thinking support
- **Nim** - NVIDIA NIM endpoints

Each provider implements:
- Non-streaming `query()` for simple requests
- Streaming `query_streaming()` with separate text/thinking callbacks
- Usage tracking (tokens, cost)

The OpenAI provider supports:
- Local llama.cpp servers (default)
- Any OpenAI-compatible API
- Optional API key for remote services

### Configuration

Config loading priority:
1. `./codr.toml` (current directory)
2. `~/.config/codr/config.toml` (XDG config home)
3. Default configuration (OpenAI on localhost:8080)

**Example `codr.toml`:**
```toml
model = "openai"  # or "anthropic" or "nim"

[openai]
base_url = "http://localhost:8080"  # llama.cpp default
model = "default"
api_key = "..."  # optional, for remote APIs

[anthropic]
api_key = "sk-ant-..."  # or set ANTHROPIC_API_KEY env var

[nim]
base_url = "https://integrate.api.nvidia.com"
model = "meta/llama-3.1-70b-instruct"
api_key = "..."  # or set NVIDIA_API_KEY env var
```

For local llama.cpp usage, start the server first:
```bash
llama-server --model /path/to/your/model.gguf
```

Then codr will connect to `http://localhost:8080` by default.

### System Prompt

The system prompt is dynamically generated to include tool descriptions and enforces:
- Use tools only for coding tasks (read, edit, run commands)
- Respond directly to greetings and casual questions
- Stop after task completion (wait for next instruction)

## Important Implementation Notes

### Streaming Display Logic

When working with streaming content:
- **Thinking** is flushed to messages on newlines (via `StreamingThinkingChunk` handler)
- **Content** is accumulated and shown in real-time (via `StreamingChunk` handler)
- **Tool-action flicker prevention**: Chunks are cleaned via `clean_streaming_chunk()` before adding to `streaming_content` to prevent brief display of tool-action blocks
- **On Done**, remaining content is flushed to a final message
- **On Disconnect**, same flushing happens (handles abrupt termination)

### Tool-Action Block Filtering

The `clean_content()` function in `tui_components.rs` removes ````tool-action` and ````bash-action` blocks from display. When modifying message rendering, ensure this filtering is preserved.

### Cancellation

Cancellation (`Ctrl+C`) works by:
1. First press: Cancel current agent operation
2. Second press (within 2 seconds): Quit application
3. Implementation: `cancel_token.cancel()`, drops `update_rx` channel, clears streaming buffers

### TUI Keybindings

**Input Navigation:**
- `Up/Down` - Navigate prompt history
- `Left/Right` - Move cursor
- `Enter` - Insert newline
- `Backspace` - Delete character
- Typing exits history mode

**Sending Messages:**
- `Ctrl+S` - Send message (when not approving)
- `[a]` - Approve bash command (when pending)
- `[r]` - Reject bash command (when pending)

**Other:**
- `Ctrl+Q` - Quit application
- `Ctrl+C` - Cancel agent (first press) / Quit (second press within 2s)
- `Ctrl+O` - Copy selection or all messages to clipboard
- `Ctrl+Y` / `Ctrl+Shift+V` - Paste from clipboard
- `Ctrl+Home` - Scroll to top
- `Ctrl+End` - Scroll to bottom
- `Ctrl+Up/Down` - Scroll by 3 lines
- `Page Up/Down` - Page scroll
- `Mouse wheel` - Scroll
- `Mouse click+drag` - Select text for copying
- `Escape` - Clear selection

### Message Rendering

`render_message()` in `tui_components.rs` handles:
- **Thinking content** - Displayed in italic, prefixed with "Thinking: "
- **Markdown rendering** - Headers, code blocks with language labels, inline code, bold
- **Role-based styling** - Different colors for user, assistant, action, output, error

### Adding New Tools

To add a new tool:
1. Implement the `Tool` trait in `src/tools/impl.rs`
2. Register it in `create_coding_tools()` in `src/tools/mod.rs`
3. Update system prompt if needed (via tools_description)

**Tool Validation:**
Tools should validate their parameters to catch malformed LLM output:
- Check for template syntax like `{pattern}`, `{file}`
- Detect incomplete JSON or unmatched braces
- Return clear error messages instead of executing unsafe commands

### Error Handling

The agent uses two error types:
- `AgentError::Timeout` - Command timeout/retry (feeds error back to LLM)
- `AgentError::Terminating` - Fatal error (ends the conversation)
